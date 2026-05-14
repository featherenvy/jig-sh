#[cfg(any(target_os = "macos", target_os = "linux"))]
use std::ffi::OsString;
use std::fs;
use std::io::{ErrorKind, Read, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, TcpListener};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
#[cfg(any(target_os = "macos", target_os = "linux"))]
use std::process::{Command, ExitStatus, Output, Stdio};
#[cfg(any(target_os = "macos", target_os = "linux"))]
use std::time::{Duration as StdDuration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use rcgen::{
    BasicConstraints, CertificateParams, CidrSubnet, DistinguishedName, DnType,
    ExtendedKeyUsagePurpose, GeneralSubtree, IsCa, Issuer, KeyPair, KeyUsagePurpose,
    NameConstraints, SerialNumber,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
#[cfg(target_os = "macos")]
use sha1::Sha1;
use sha2::{Digest, Sha256};
use time::{Duration as TimeDuration, OffsetDateTime};
use x509_parser::oid_registry::{
    OID_EC_P256, OID_KEY_TYPE_EC_PUBLIC_KEY, OID_SIG_ECDSA_WITH_SHA256,
};
use x509_parser::parse_x509_certificate;
use zeroize::Zeroizing;

use crate::file_ops;
use crate::host::{validate_hostname, validate_routed_hostname, validate_tld};
use crate::ports::local_lan_ip_for_ipv4_listener;
use crate::state::StateStore;
use crate::types::ProxySettings;

const CA_VALIDITY_DAYS: i64 = 730;
const LEAF_VALIDITY_DAYS: i64 = 200;
const LEAF_HOSTS_VERSION: u32 = 1;
const TRUSTED_CA_VERSION: u32 = 1;
const MAX_CERTIFICATE_HOSTS: usize = 1024;
const MAX_CERT_PEM_BYTES: u64 = 1024 * 1024;
const MAX_PRIVATE_KEY_PEM_BYTES: u64 = 256 * 1024;
const CERT_FILE_FALLBACK: &str = "jig-proxy-cert";
#[cfg(target_os = "linux")]
const MAX_TRUST_BUNDLE_PEM_BYTES: u64 = 16 * 1024 * 1024;
#[cfg(any(target_os = "macos", target_os = "linux"))]
const TRUST_COMMAND_TIMEOUT: StdDuration = StdDuration::from_secs(30);
const CA_SERIAL_NUMBER_BYTES: usize = 16;
const JIG_CA_COMMON_NAME: &str = "Jig Dev Proxy Local CA";
const GLOBAL_CA_TRUST_WARNING: &str = "Trusting the Jig Dev Proxy local CA installs a locally trusted root constrained to configured Jig development DNS names and loopback/IPv4 LAN IP addresses; keep ca-key.pem private because any locally trusted CA key is sensitive machine-local TLS material.";
#[cfg(any(target_os = "macos", test))]
const MACOS_UNTRUST_REMOVAL_LIMIT: usize = 64;

pub(crate) fn generate(settings: &ProxySettings, force: bool) -> Result<Value> {
    ensure_certificate_generation_supported()?;
    let store = StateStore::resolve(settings.state_dir.clone())?;
    store.with_cert_lock(|| {
        remove_stale_cert_temps(&store)?;
        let files = cert_file_state(&store);
        let ca_pair_invalid = ca_pair_invalid(&store, files, settings);
        let leaf_pair_invalid = leaf_pair_invalid(&store, files);
        if !force && files.is_complete() && !ca_pair_invalid && !leaf_pair_invalid {
            bail!(
                "Proxy certificates already exist in {}. Pass --force to regenerate them.",
                store.root().display()
            );
        }

        if force || files.ca_pair_is_partial() || ca_pair_invalid {
            ensure_ca_can_be_replaced(&store)?;
        }
        if force || !files.ca_exists || !files.ca_key_exists || ca_pair_invalid {
            write_ca(&store, settings)?;
        }
        let hosts = certificate_hosts(settings, &store, &[])?;
        write_leaf(&store, &hosts)?;

        warn_global_ca_trust(&store);
        Ok(certificate_paths(&store))
    })
}

pub(crate) fn ensure(settings: &ProxySettings) -> Result<Value> {
    ensure_for_hosts(settings, &[])
}

pub(crate) fn ensure_for_hosts(settings: &ProxySettings, hostnames: &[String]) -> Result<Value> {
    ensure_certificate_generation_supported()?;
    let store = StateStore::resolve(settings.state_dir.clone())?;
    store.with_cert_lock(|| {
        remove_stale_cert_temps(&store)?;
        let files = cert_file_state(&store);
        let ca_pair_invalid = ca_pair_invalid(&store, files, settings);
        if files.ca_pair_is_partial() || ca_pair_invalid {
            ensure_ca_can_be_replaced(&store)?;
        }
        // Existing valid CAs, including ones trusted by Jig, may continue to
        // issue refreshed leaf certificates. Only CA replacement paths require
        // untrusting first so old trusted roots are not orphaned.
        if !files.ca_exists || !files.ca_key_exists || ca_pair_invalid {
            write_ca(&store, settings)?;
        }
        let hosts = certificate_hosts(settings, &store, hostnames)?;
        ensure_leaf_hosts_within_ca_constraints(&store.ca_path(), &hosts)?;
        if leaf_matches_hosts(&store, &hosts)? {
            restrict_private_key(&store.ca_key_path())?;
            restrict_private_key(&store.leaf_key_path())?;
            return Ok(certificate_paths(&store));
        }
        write_leaf(&store, &hosts)?;
        Ok(certificate_paths(&store))
    })
}

fn ensure_certificate_generation_supported() -> Result<()> {
    #[cfg(windows)]
    {
        bail!(
            "TLS certificate generation is not supported on Windows until owner-only private-key ACL hardening is implemented; use macOS or Linux for automatic HTTPS certificate generation."
        );
    }
    #[cfg(not(windows))]
    {
        Ok(())
    }
}

fn write_ca(store: &StateStore, settings: &ProxySettings) -> Result<()> {
    let ca_params = ca_params(settings)?;
    let ca_key = KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256)?;
    let ca = ca_params.self_signed(&ca_key)?;
    let ca_key_pem = Zeroizing::new(ca_key.serialize_pem());

    write_private_key(store.ca_key_path(), &ca_key_pem)?;
    write_public_pem(store.ca_path(), &ca.pem())?;
    remove_leaf_material_after_ca_rotation(store)?;
    Ok(())
}

fn remove_leaf_material_after_ca_rotation(store: &StateStore) -> Result<()> {
    remove_optional_cert_file(store.leaf_path())?;
    remove_optional_cert_file(store.leaf_key_path())?;
    remove_optional_cert_file(store.leaf_hosts_path())?;
    Ok(())
}

fn remove_optional_cert_file(path: PathBuf) -> Result<()> {
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error)
            .with_context(|| format!("Failed to remove stale certificate file {}", path.display())),
    }
}

fn ensure_ca_can_be_replaced(store: &StateStore) -> Result<()> {
    if trusted_ca_marker_matches(store)? {
        bail!(
            "Current Jig proxy CA was trusted by Jig. Run `scripts/jig proxy cert untrust --accept-trust-scope` before regenerating it with --force so the old trusted root is not orphaned."
        );
    }
    #[cfg(target_os = "macos")]
    {
        if store.ca_path().exists() {
            let fingerprints = ca_fingerprints(&store.ca_path()).with_context(|| {
                format!(
                    "Failed to inspect current Jig proxy CA certificate {} before replacing it",
                    store.ca_path().display()
                )
            })?;
            if macos_trusted_ca_fingerprint_exists(&fingerprints).context(
                "Failed to inspect macOS trust store before replacing current Jig proxy CA",
            )? {
                bail!(
                    "Current Jig proxy CA appears to be trusted. Run `scripts/jig proxy cert untrust --accept-trust-scope` before regenerating it with --force so the old trusted root is not orphaned."
                );
            }
        }
    }
    #[cfg(target_os = "linux")]
    {
        if store.ca_path().exists() && linux_current_jig_ca_is_trusted(store)? {
            bail!(
                "A Jig Dev Proxy Local CA appears to be trusted by the Linux trust store. Run `scripts/jig proxy cert untrust --accept-trust-scope` before regenerating it with --force so the old trusted root is not orphaned."
            );
        }
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct CertFileState {
    ca_exists: bool,
    ca_key_exists: bool,
    leaf_exists: bool,
    leaf_key_exists: bool,
}

impl CertFileState {
    fn is_complete(self) -> bool {
        self.ca_exists && self.ca_key_exists && self.leaf_exists && self.leaf_key_exists
    }

    fn ca_pair_is_partial(self) -> bool {
        self.ca_exists != self.ca_key_exists
    }
}

fn cert_file_state(store: &StateStore) -> CertFileState {
    CertFileState {
        ca_exists: store.ca_path().exists(),
        ca_key_exists: store.ca_key_path().exists(),
        leaf_exists: store.leaf_path().exists(),
        leaf_key_exists: store.leaf_key_path().exists(),
    }
}

fn ca_pair_invalid(store: &StateStore, files: CertFileState, settings: &ProxySettings) -> bool {
    if !files.ca_exists || !files.ca_key_exists {
        return false;
    }
    match certificate_is_current(&store.ca_path()) {
        Ok(true) => {}
        Ok(false) => return true,
        Err(error) => {
            eprintln!(
                "jig proxy CA certificate validity could not be checked; regenerating it: {error:#}"
            );
            return true;
        }
    }
    match ca_name_constraints_cover_settings(&store.ca_path(), settings) {
        Ok(true) => {}
        Ok(false) => return true,
        Err(error) => {
            eprintln!(
                "jig proxy CA certificate name constraints could not be checked; regenerating it: {error:#}"
            );
            return true;
        }
    }
    match private_key_matches_certificate(&store.ca_key_path(), &store.ca_path()) {
        Ok(true) => false,
        Ok(false) => true,
        Err(error) => {
            eprintln!("jig proxy CA key/certificate pair is invalid; regenerating it: {error:#}");
            true
        }
    }
}

fn ca_name_constraints_cover_settings(path: &Path, settings: &ProxySettings) -> Result<bool> {
    let der = first_certificate_der(path)?;
    let (_, cert) = parse_x509_certificate(&der)
        .map_err(|error| anyhow!("Failed to parse Jig proxy CA certificate DER: {error}"))?;
    let Some(constraints) = cert
        .name_constraints()
        .map_err(|error| anyhow!("Failed to parse Jig proxy CA name constraints: {error}"))?
    else {
        return Ok(false);
    };
    let Some(permitted) = &constraints.value.permitted_subtrees else {
        return Ok(false);
    };
    let (required_dns, required_ip_constraints) = required_ca_name_constraints(settings)?;
    let dns_covered = required_dns.iter().all(|required| {
        permitted.iter().any(|subtree| {
            matches!(
                subtree.base,
                x509_parser::extensions::GeneralName::DNSName(name)
                    if dns_constraint_covers(name, required)
            )
        })
    });
    let ip_covered = required_ip_constraints.iter().all(|required| {
        permitted.iter().any(|subtree| {
            matches!(
                subtree.base,
                x509_parser::extensions::GeneralName::IPAddress(bytes)
                    if bytes == required.as_slice()
            )
        })
    });
    Ok(dns_covered && ip_covered)
}

fn dns_constraint_covers(permitted: &str, required: &str) -> bool {
    let permitted = permitted.to_ascii_lowercase();
    let required = required.to_ascii_lowercase();
    if permitted.is_empty() {
        return false;
    }
    if permitted.starts_with('.') {
        return required.ends_with(&permitted) && required.len() > permitted.len();
    }
    required == permitted || required.ends_with(&format!(".{permitted}"))
}

fn leaf_pair_invalid(store: &StateStore, files: CertFileState) -> bool {
    if !files.leaf_exists || !files.leaf_key_exists {
        return false;
    }
    match certificate_is_current(&store.leaf_path()) {
        Ok(true) => {}
        Ok(false) => return true,
        Err(error) => {
            eprintln!(
                "jig proxy leaf certificate validity could not be checked; regenerating it: {error:#}"
            );
            return true;
        }
    }
    match private_key_matches_certificate(&store.leaf_key_path(), &store.leaf_path()) {
        Ok(true) => false,
        Ok(false) => true,
        Err(error) => {
            eprintln!("jig proxy leaf key/certificate pair is invalid; regenerating it: {error:#}");
            true
        }
    }
}

fn write_leaf(store: &StateStore, hosts: &[String]) -> Result<()> {
    ensure_leaf_hosts_within_ca_constraints(&store.ca_path(), hosts)?;
    let mut leaf_params = CertificateParams::new(hosts.to_vec())?;
    set_validity(&mut leaf_params, LEAF_VALIDITY_DAYS);
    leaf_params.distinguished_name = DistinguishedName::new();
    leaf_params
        .distinguished_name
        .push(DnType::CommonName, "Jig Dev Proxy");
    leaf_params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    let (leaf_key, leaf_key_pem) = leaf_key_for_write(store)?;
    let leaf = {
        let ca_cert_pem = fs::read_to_string(store.ca_path())?;
        let ca_key_pem = read_private_key(&store.ca_key_path())?;
        // rcgen::KeyPair does not zeroize on drop. Keep the parsed CA key in
        // the smallest practical scope; the PEM buffer itself is zeroized.
        let ca_key = KeyPair::from_pem(&ca_key_pem).context("Failed to parse Jig proxy CA key")?;
        let ca = Issuer::from_ca_cert_pem(&ca_cert_pem, ca_key)
            .context("Failed to parse Jig proxy CA certificate")?;
        leaf_params.signed_by(&leaf_key, &ca)?
    };

    write_public_pem(store.leaf_path(), &leaf.pem())?;
    if let Some(leaf_key_pem) = leaf_key_pem {
        write_private_key(store.leaf_key_path(), &leaf_key_pem)?;
    } else {
        restrict_private_key(&store.leaf_key_path())?;
    }
    write_leaf_hosts(store, hosts)?;
    Ok(())
}

fn leaf_key_for_write(store: &StateStore) -> Result<(KeyPair, Option<Zeroizing<String>>)> {
    match read_optional_private_key(&store.leaf_key_path()) {
        Ok(Some(pem)) => match KeyPair::from_pem(&pem) {
            Ok(key) => return Ok((key, None)),
            Err(error) => {
                eprintln!("jig proxy failed to parse existing leaf key; regenerating it: {error}");
            }
        },
        Ok(None) => {}
        Err(error) => return Err(error).context("Failed to read Jig proxy leaf private key"),
    }

    let leaf_key = KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256)?;
    let leaf_key_pem = Zeroizing::new(leaf_key.serialize_pem());
    Ok((leaf_key, Some(leaf_key_pem)))
}

pub(crate) fn certificate_hosts(
    settings: &ProxySettings,
    store: &StateStore,
    hostnames: &[String],
) -> Result<Vec<String>> {
    let mut hosts = vec!["localhost".to_string()];
    if settings.lan && settings.tld.eq_ignore_ascii_case("local") {
        eprintln!(
            "jig proxy LAN mode with tld=local omits the broad *.local certificate SAN; use repo-scoped route hostnames or explicit additional DNS names."
        );
    }
    hosts.extend(settings.additional_dns_names.clone());
    if settings.lan {
        if let Some(ip) = bindable_lan_ip() {
            hosts.push(ip.to_string());
        }
    }
    hosts.extend(hostnames.iter().cloned());
    // Keep certificate generation on the cert-lock -> route-lock ordering.
    // Route mutation paths do not acquire the cert lock while holding routes.
    // Do not call certificate generation or certificate refresh from inside a
    // route-lock closure; that would invert this order and can deadlock.
    hosts.extend(
        store
            .read_routes(true)?
            .into_iter()
            .map(|route| route.hostname.into_string()),
    );
    for host in &mut hosts {
        if host.parse::<IpAddr>().is_err() {
            *host = host.to_ascii_lowercase();
        }
    }
    hosts.sort();
    hosts.dedup();
    if hosts.len() > MAX_CERTIFICATE_HOSTS {
        bail!(
            "Jig proxy certificate would contain {} SAN entries, above the limit of {MAX_CERTIFICATE_HOSTS}. Prune stale routes or use a dedicated JIG_PROXY_STATE_DIR.",
            hosts.len()
        );
    }
    for host in &hosts {
        validate_certificate_host(host)?;
    }
    Ok(hosts)
}

fn validate_certificate_host(host: &str) -> Result<()> {
    if host.parse::<IpAddr>().is_ok() {
        return Ok(());
    }
    if let Some(rest) = host.strip_prefix("*.") {
        validate_hostname(rest)
    } else {
        validate_hostname(host)
    }
}

fn ensure_leaf_hosts_within_ca_constraints(ca_path: &Path, hosts: &[String]) -> Result<()> {
    let der = first_certificate_der(ca_path)?;
    let (_, cert) = parse_x509_certificate(&der)
        .map_err(|error| anyhow!("Failed to parse Jig proxy CA certificate DER: {error}"))?;
    let Some(constraints) = cert
        .name_constraints()
        .map_err(|error| anyhow!("Failed to parse Jig proxy CA name constraints: {error}"))?
    else {
        bail!(
            "Jig proxy CA certificate has no name constraints; refusing to issue a leaf certificate"
        );
    };
    let Some(permitted) = &constraints.value.permitted_subtrees else {
        bail!(
            "Jig proxy CA certificate has no permitted name constraints; refusing to issue a leaf certificate"
        );
    };
    for host in hosts {
        let allowed = if let Ok(ip) = host.parse::<IpAddr>() {
            permitted.iter().any(|subtree| {
                matches!(
                    subtree.base,
                    x509_parser::extensions::GeneralName::IPAddress(bytes)
                        if ip_constraint_covers(bytes, ip)
                )
            })
        } else {
            permitted.iter().any(|subtree| {
                matches!(
                    subtree.base,
                    x509_parser::extensions::GeneralName::DNSName(name)
                        if dns_constraint_covers(name, host)
                )
            })
        };
        if !allowed {
            bail!("Jig proxy leaf certificate host '{host}' is outside the CA name constraints");
        }
    }
    Ok(())
}

fn ip_constraint_covers(constraint: &[u8], ip: IpAddr) -> bool {
    match (ip, constraint.len()) {
        (IpAddr::V4(ip), 8) => ip_matches_mask(&ip.octets(), &constraint[..4], &constraint[4..]),
        (IpAddr::V4(ip), 4) => constraint == ip.octets(),
        (IpAddr::V6(ip), 32) => ip_matches_mask(&ip.octets(), &constraint[..16], &constraint[16..]),
        (IpAddr::V6(ip), 16) => constraint == ip.octets(),
        _ => false,
    }
}

fn ip_matches_mask(ip: &[u8], base: &[u8], mask: &[u8]) -> bool {
    ip.len() == base.len()
        && base.len() == mask.len()
        && ip
            .iter()
            .zip(base)
            .zip(mask)
            .all(|((ip, base), mask)| (ip & mask) == (base & mask))
}

fn write_public_pem(path: PathBuf, pem: &str) -> Result<()> {
    file_ops::write_atomic_text(path, pem, CERT_FILE_FALLBACK)
}

fn write_leaf_hosts(store: &StateStore, hosts: &[String]) -> Result<()> {
    let record = LeafHostsRecord {
        version: LEAF_HOSTS_VERSION,
        hosts: hosts.to_vec(),
        ca_hash: file_hash(&store.ca_path()).context("Failed to hash Jig proxy CA certificate")?,
        certificate_hash: file_hash(&store.leaf_path())
            .context("Failed to hash Jig proxy leaf certificate")?,
    };
    let text = serde_json::to_string_pretty(&record)?;
    file_ops::write_atomic_text(store.leaf_hosts_path(), &text, CERT_FILE_FALLBACK)
}

fn leaf_matches_hosts(store: &StateStore, desired_hosts: &[String]) -> Result<bool> {
    // This sidecar is written in the same locked regeneration path as the leaf
    // certificate. Treat parse/read failure as a cache miss and regenerate.
    if !store.leaf_path().exists()
        || !store.leaf_key_path().exists()
        || !store.leaf_hosts_path().exists()
    {
        return Ok(false);
    }
    let Ok(text) = fs::read_to_string(store.leaf_hosts_path()) else {
        return Ok(false);
    };
    let Ok(existing) = serde_json::from_str::<LeafHostsFile>(&text) else {
        return Ok(false);
    };
    match existing {
        LeafHostsFile::Record(record) if record.version == LEAF_HOSTS_VERSION => {
            if !hosts_cover(&record.hosts, desired_hosts)
                || Some(record.ca_hash) != file_hash(&store.ca_path())
                || Some(record.certificate_hash) != file_hash(&store.leaf_path())
                || !certificate_is_current(&store.ca_path()).unwrap_or(false)
                || !certificate_is_current(&store.leaf_path()).unwrap_or(false)
            {
                return Ok(false);
            }
            Ok(
                private_key_matches_certificate(&store.leaf_key_path(), &store.leaf_path())
                    .unwrap_or(false),
            )
        }
        LeafHostsFile::Record(_) => Ok(false),
        LeafHostsFile::Legacy(hosts) => {
            let _ = hosts;
            Ok(false)
        }
    }
}

fn hosts_cover(existing_hosts: &[String], desired_hosts: &[String]) -> bool {
    desired_hosts
        .iter()
        .all(|host| existing_hosts.iter().any(|existing| existing == host))
}

fn certificate_is_current(path: &Path) -> Result<bool> {
    let der = first_certificate_der(path)?;
    let (_, cert) = parse_x509_certificate(&der)
        .map_err(|error| anyhow!("Failed to parse certificate DER: {error}"))?;
    Ok(cert.validity().is_valid())
}

#[derive(Deserialize)]
#[serde(untagged)]
enum LeafHostsFile {
    Record(LeafHostsRecord),
    Legacy(Vec<String>),
}

#[derive(Deserialize, Serialize)]
struct LeafHostsRecord {
    version: u32,
    hosts: Vec<String>,
    ca_hash: String,
    certificate_hash: String,
}

#[derive(Deserialize, Serialize)]
struct TrustedCaRecord {
    version: u32,
    platform: String,
    ca_hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ca_fingerprint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ca_sha256_fingerprint: Option<String>,
}

fn write_private_key(path: PathBuf, pem: &str) -> Result<()> {
    write_owner_only_text(path, pem)
}

fn write_owner_only_text(path: PathBuf, contents: &str) -> Result<()> {
    #[cfg(unix)]
    {
        let tmp = file_ops::temp_path(&path, CERT_FILE_FALLBACK);
        let mut file = file_ops::create_new_file(&tmp, 0o600)?;
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
        file.write_all(contents.as_bytes())?;
        file.sync_data()?;
        drop(file);
        file_ops::replace_file(&tmp, &path, CERT_FILE_FALLBACK)?;
        Ok(())
    }

    #[cfg(windows)]
    {
        let _ = (path, contents);
        bail!(
            "Jig proxy TLS certificate generation is not supported on Windows until owner-only private-key ACL hardening is implemented; use macOS or Linux for HTTPS proxy certificates"
        )
    }

    #[cfg(not(any(unix, windows)))]
    {
        let _ = (path, contents);
        bail!("Jig proxy private key writes are not supported on this platform")
    }
}

fn read_private_key(path: &Path) -> Result<Zeroizing<String>> {
    read_optional_private_key(path)?
        .with_context(|| format!("Failed to read Jig proxy private key {}", path.display()))
}

fn read_optional_private_key(path: &Path) -> Result<Option<Zeroizing<String>>> {
    let Some(mut file) = open_optional_read_no_follow(path, MAX_PRIVATE_KEY_PEM_BYTES)? else {
        return Ok(None);
    };
    ensure_private_key_permissions(path, &file)?;
    let mut pem = Zeroizing::new(String::new());
    file.read_to_string(&mut pem)?;
    Ok(Some(pem))
}

fn ensure_private_key_permissions(path: &Path, file: &fs::File) -> Result<()> {
    #[cfg(unix)]
    {
        let mode = file.metadata()?.permissions().mode() & 0o7777;
        if mode != 0o600 {
            bail!(
                "Refusing to read Jig proxy private key {} with permissions {:o}; tighten it to mode 600 first.",
                path.display(),
                mode
            );
        }
    }
    #[cfg(not(unix))]
    let _ = (path, file);
    Ok(())
}

fn restrict_private_key(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        let file = match file_ops::open_read_no_follow(path) {
            Ok(file) => file,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error.into()),
        };
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

fn ca_params(settings: &ProxySettings) -> Result<CertificateParams> {
    let mut params = CertificateParams::default();
    params.serial_number = Some(random_serial_number()?);
    set_validity(&mut params, CA_VALIDITY_DAYS);
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    params.name_constraints = Some(ca_name_constraints(settings)?);
    params.key_usages = vec![
        KeyUsagePurpose::KeyCertSign,
        KeyUsagePurpose::CrlSign,
        KeyUsagePurpose::DigitalSignature,
    ];
    params.distinguished_name = DistinguishedName::new();
    params
        .distinguished_name
        .push(DnType::CommonName, JIG_CA_COMMON_NAME);
    Ok(params)
}

fn ca_name_constraints(settings: &ProxySettings) -> Result<NameConstraints> {
    let mut permitted_subtrees = Vec::new();
    for dns_name in ca_dns_name_constraints(settings)? {
        permitted_subtrees.push(GeneralSubtree::DnsName(dns_name));
    }
    for (ip, prefix) in ca_ip_name_constraints(settings) {
        permitted_subtrees.push(GeneralSubtree::IpAddress(CidrSubnet::from_addr_prefix(
            ip, prefix,
        )));
    }
    permitted_subtrees.sort_by_key(|subtree| format!("{subtree:?}"));
    permitted_subtrees.dedup();
    Ok(NameConstraints {
        permitted_subtrees,
        excluded_subtrees: Vec::new(),
    })
}

fn required_ca_name_constraints(settings: &ProxySettings) -> Result<(Vec<String>, Vec<Vec<u8>>)> {
    let dns_names = ca_dns_name_constraints(settings)?;
    let ip_constraints = ca_ip_name_constraints(settings)
        .into_iter()
        .map(|(ip, prefix)| ip_name_constraint_bytes(ip, prefix))
        .collect();
    Ok((dns_names, ip_constraints))
}

fn ca_dns_name_constraints(settings: &ProxySettings) -> Result<Vec<String>> {
    validate_tld(&settings.tld).with_context(|| {
        format!(
            "Invalid dev proxy TLD '{}' for CA name constraints",
            settings.tld
        )
    })?;
    let tld = settings.tld.to_ascii_lowercase();
    let mut dns_names = vec!["localhost".to_string()];
    if tld == "localhost" {
        dns_names.push(tld);
    }
    for name in &settings.additional_dns_names {
        if name.parse::<IpAddr>().is_err() {
            let name = name.strip_prefix("*.").unwrap_or(name).to_ascii_lowercase();
            validate_routed_hostname(&name).with_context(|| {
                format!(
                    "Additional DNS name '{name}' must be a configured Jig development DNS name"
                )
            })?;
            dns_names.push(name);
        }
    }
    dns_names.sort();
    dns_names.dedup();
    Ok(dns_names)
}

fn ca_ip_name_constraints(settings: &ProxySettings) -> Vec<(IpAddr, u8)> {
    let mut ip_constraints = vec![
        (IpAddr::V4(Ipv4Addr::new(127, 0, 0, 0)), 8),
        (IpAddr::V6(Ipv6Addr::LOCALHOST), 128),
    ];
    ip_constraints.extend(settings.additional_dns_names.iter().filter_map(|name| {
        let ip = name.parse::<IpAddr>().ok()?;
        let prefix = match ip {
            IpAddr::V4(_) => 32,
            IpAddr::V6(_) => 128,
        };
        Some((ip, prefix))
    }));
    if settings.lan {
        if let Some(ip) = bindable_lan_ip() {
            let prefix = match ip {
                IpAddr::V4(_) => 32,
                IpAddr::V6(_) => 128,
            };
            ip_constraints.push((ip, prefix));
        }
    }
    ip_constraints.sort();
    ip_constraints.dedup();
    ip_constraints
}

fn ip_name_constraint_bytes(ip: IpAddr, prefix: u8) -> Vec<u8> {
    let (mut bytes, len) = match ip {
        IpAddr::V4(ip) => (ip.octets().to_vec(), 4usize),
        IpAddr::V6(ip) => (ip.octets().to_vec(), 16usize),
    };
    bytes.extend(prefix_mask_bytes(len, prefix));
    bytes
}

fn prefix_mask_bytes(len: usize, prefix: u8) -> Vec<u8> {
    let mut mask = vec![0u8; len];
    for bit in 0..usize::from(prefix).min(len * 8) {
        mask[bit / 8] |= 0x80 >> (bit % 8);
    }
    mask
}

fn bindable_lan_ip() -> Option<IpAddr> {
    local_lan_ip_for_ipv4_listener().filter(|ip| TcpListener::bind((*ip, 0)).is_ok())
}

fn random_serial_number() -> Result<SerialNumber> {
    let mut bytes = [0u8; CA_SERIAL_NUMBER_BYTES];
    getrandom::fill(&mut bytes)
        .map_err(|error| anyhow!("Failed to generate random CA serial number: {error}"))?;
    // X.509 serials are positive DER integers. Keep the high bit clear and
    // the first byte nonzero so DER preserves the full 16-byte serial while
    // retaining roughly 127 bits of entropy.
    bytes[0] = (bytes[0] & 0x7f) | 0x01;
    Ok(SerialNumber::from_slice(&bytes))
}

fn set_validity(params: &mut CertificateParams, days: i64) {
    let not_before = OffsetDateTime::now_utc() - TimeDuration::days(1);
    params.not_before = not_before;
    params.not_after = not_before + TimeDuration::days(days);
}

fn remove_stale_cert_temps(store: &StateStore) -> Result<()> {
    for entry in fs::read_dir(store.root())? {
        let entry = entry?;
        let path = entry.path();
        if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(is_cert_temp_name)
        {
            if let Err(error) = fs::remove_file(&path) {
                eprintln!(
                    "jig proxy could not remove stale certificate temp file {}: {error}",
                    path.display()
                );
            }
        }
    }
    Ok(())
}

fn is_cert_temp_name(name: &str) -> bool {
    [
        "ca.pem.",
        "ca-key.pem.",
        "leaf.pem.",
        "leaf-key.pem.",
        "leaf-hosts.json.",
        "trusted-ca.json.",
        "trusted-anchors.pem.",
    ]
    .iter()
    .any(|prefix| name.strip_prefix(prefix).is_some_and(generated_temp_suffix))
}

fn generated_temp_suffix(suffix: &str) -> bool {
    let mut parts = suffix.split('.');
    let Some(pid) = parts.next() else {
        return false;
    };
    let Some(timestamp_ms) = parts.next() else {
        return false;
    };
    let Some(counter) = parts.next() else {
        return false;
    };
    matches!(parts.next(), Some("tmp"))
        && parts.next().is_none()
        && !pid.is_empty()
        && !timestamp_ms.is_empty()
        && !counter.is_empty()
        && pid.bytes().all(|ch| ch.is_ascii_digit())
        && timestamp_ms.bytes().all(|ch| ch.is_ascii_digit())
        && counter.bytes().all(|ch| ch.is_ascii_digit())
}

fn file_hash(path: &Path) -> Option<String> {
    let mut file = open_optional_read_no_follow(path, MAX_CERT_PEM_BYTES)
        .ok()
        .flatten()?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).ok()?;
    Some(hex_lower(&Sha256::digest(bytes)))
}

fn certificate_paths(store: &StateStore) -> Value {
    json!({
        "ok": true,
        "state_dir": store.root(),
        "ca": store.ca_path(),
        "certificate": store.leaf_path(),
        "key": store.leaf_key_path(),
        "trust_warning": GLOBAL_CA_TRUST_WARNING,
    })
}

pub(crate) fn status(settings: &ProxySettings) -> Result<Value> {
    let store = StateStore::resolve(settings.state_dir.clone())?;
    warn_global_ca_trust(&store);
    Ok(json!({
        "ok": true,
        "state_dir": store.root(),
        "ca_exists": store.ca_path().exists(),
        "certificate_exists": store.leaf_path().exists(),
        "key_exists": store.leaf_key_path().exists(),
        "trust_check": trust_check(&store),
        "trust_warning": GLOBAL_CA_TRUST_WARNING,
    }))
}

pub(crate) fn trust(settings: &ProxySettings, accept_trust_scope: bool) -> Result<Value> {
    let store = StateStore::resolve(settings.state_dir.clone())?;
    store.with_cert_lock(|| trust_locked(&store, accept_trust_scope))
}

fn trust_locked(store: &StateStore, accept_trust_scope: bool) -> Result<Value> {
    if !store.ca_path().exists() {
        bail!("CA certificate does not exist. Run `scripts/jig proxy cert generate` first.");
    }
    ensure_jig_ca_certificate(store)?;
    ensure_jig_ca_private_key(store)?;
    if !accept_trust_scope {
        bail!(
            "Refusing to trust the Jig Dev Proxy local CA without --accept-trust-scope. {}",
            GLOBAL_CA_TRUST_WARNING
        );
    }
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    warn_global_ca_trust(store);

    #[cfg(target_os = "macos")]
    {
        let keychain = login_keychain_path()?;
        let mut command = macos_security_command();
        command
            .args(["add-trusted-cert", "-r", "trustRoot", "-k"])
            .arg(&keychain)
            .arg(command_path_arg(&store.ca_path()));
        let status = command_status_with_timeout(&mut command, "security add-trusted-cert")?;
        if !status.success() {
            bail!("security add-trusted-cert failed with status {status}");
        }
        ensure_macos_current_ca_is_trusted(store)?;
        // The marker is written only after platform trust succeeds. If the
        // process crashes between those steps, untrust falls back to scanning
        // Jig-labelled trusted roots instead of trusting only the marker.
        write_trusted_ca_marker(store)?;
        Ok(json!({
            "ok": true,
            "trusted": true,
            "platform": "macos",
            "warning": GLOBAL_CA_TRUST_WARNING,
        }))
    }

    #[cfg(target_os = "linux")]
    {
        if linux_command_available("trust") {
            let mut command = linux_system_command("trust")?;
            command
                .arg("anchor")
                .arg(command_path_arg(&store.ca_path()));
            let status = command_status_with_timeout(&mut command, "trust anchor")?;
            if !status.success() {
                bail!("trust anchor failed with status {status}");
            }
            let bundle_update = match linux_refresh_ca_bundles() {
                Ok(value) => value,
                Err(error) => {
                    return Err(error).context(
                        "trust anchor succeeded, but refreshing system CA bundles failed",
                    );
                }
            };
            // The marker is written only after platform trust succeeds. If the
            // process crashes between those steps, untrust falls back to
            // scanning Jig-labelled trusted roots instead of trusting only the marker.
            write_trusted_ca_marker(store)?;
            Ok(json!({
                "ok": true,
                "trusted": true,
                "platform": "linux",
                "system_bundle_update": bundle_update,
                "warning": GLOBAL_CA_TRUST_WARNING,
            }))
        } else {
            bail!(
                "Automatic trust requires the `trust` command. Install p11-kit or import {} manually.",
                store.ca_path().display()
            );
        }
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    bail!("Automatic certificate trust is not supported on this platform.");
}

fn warn_global_ca_trust(store: &StateStore) {
    eprintln!(
        "Warning: {} CA path: {}",
        GLOBAL_CA_TRUST_WARNING,
        store.ca_path().display()
    );
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn command_path_arg(path: &Path) -> OsString {
    if !path.is_absolute() && path.to_string_lossy().starts_with('-') {
        return Path::new(".").join(path).into_os_string();
    }
    path.as_os_str().to_owned()
}

#[cfg(target_os = "macos")]
fn macos_security_command() -> Command {
    let mut command = Command::new("/usr/bin/security");
    command.env_clear();
    command
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn command_status_with_timeout(command: &mut Command, action: &str) -> Result<ExitStatus> {
    command.stdin(Stdio::null());
    let mut child = command
        .spawn()
        .with_context(|| format!("Failed to run {action}"))?;
    wait_child_with_timeout(&mut child, action)
}

#[cfg(target_os = "macos")]
fn ensure_macos_current_ca_is_trusted(store: &StateStore) -> Result<()> {
    let fingerprints = ca_fingerprints(&store.ca_path()).with_context(|| {
        format!(
            "Failed to inspect Jig proxy CA certificate {} after trust install",
            store.ca_path().display()
        )
    })?;
    if !macos_trusted_ca_fingerprint_exists(&fingerprints)
        .context("Failed to verify macOS trust store after installing Jig proxy CA")?
    {
        bail!(
            "security add-trusted-cert completed, but the Jig proxy CA was not found in the macOS trust store"
        );
    }
    Ok(())
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn command_output_with_timeout(command: &mut Command, action: &str) -> Result<Output> {
    let temp_dir = std::env::temp_dir();
    // TMPDIR is allowed to redirect these captures. The files themselves are
    // still created with O_NOFOLLOW | O_EXCL and mode 0600 by file_ops.
    fs::create_dir_all(&temp_dir).with_context(|| {
        format!(
            "Failed to create temporary command-output directory {}",
            temp_dir.display()
        )
    })?;
    let temp_base = temp_dir.join("jig-proxy-command-output");
    let stdout_path = file_ops::temp_path(&temp_base, "jig-proxy-command-output");
    let stderr_path = file_ops::temp_path(&temp_base, "jig-proxy-command-output");
    let stdout_file = file_ops::create_new_file(&stdout_path, 0o600)
        .with_context(|| format!("Failed to create temporary stdout file for {action}"))?;
    let stderr_file = file_ops::create_new_file(&stderr_path, 0o600)
        .with_context(|| format!("Failed to create temporary stderr file for {action}"))?;
    command
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout_file))
        .stderr(Stdio::from(stderr_file));
    let mut child = command
        .spawn()
        .with_context(|| format!("Failed to run {action}"))?;
    let status = wait_child_with_timeout(&mut child, action);
    let stdout = fs::read(&stdout_path).unwrap_or_default();
    let stderr = fs::read(&stderr_path).unwrap_or_default();
    let _ = fs::remove_file(&stdout_path);
    let _ = fs::remove_file(&stderr_path);
    Ok(Output {
        status: status?,
        stdout,
        stderr,
    })
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn wait_child_with_timeout(child: &mut std::process::Child, action: &str) -> Result<ExitStatus> {
    let deadline = Instant::now() + TRUST_COMMAND_TIMEOUT;
    loop {
        if let Some(status) = child
            .try_wait()
            .with_context(|| format!("Failed to wait for {action}"))?
        {
            return Ok(status);
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            bail!("{action} timed out after {:?}", TRUST_COMMAND_TIMEOUT);
        }
        std::thread::sleep(StdDuration::from_millis(50));
    }
}

pub(crate) fn untrust(settings: &ProxySettings, accept_trust_scope: bool) -> Result<Value> {
    if !accept_trust_scope {
        bail!(
            "Refusing to mutate platform trust settings without --accept-trust-scope. This command removes matching Jig Dev Proxy local CA certificates from the platform trust store."
        );
    }
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        let store = StateStore::resolve(settings.state_dir.clone())?;
        store.with_cert_lock(|| untrust_locked(&store))
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = settings;
        bail!("Automatic certificate untrust is not supported on this platform.");
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn untrust_locked(store: &StateStore) -> Result<Value> {
    #[cfg(target_os = "macos")]
    {
        let target_fingerprint = if let Some(fingerprint) = trusted_ca_marker_fingerprints(store)? {
            Some(fingerprint)
        } else if store.ca_path().exists() {
            ensure_jig_ca_certificate(store)?;
            Some(ca_fingerprints(&store.ca_path())?)
        } else {
            None
        };
        let mut removed = 0usize;
        if let Some(target_fingerprint) = target_fingerprint {
            while removed < MACOS_UNTRUST_REMOVAL_LIMIT {
                let fingerprints = macos_trusted_jig_ca_fingerprints()?;
                let Some(fingerprint) = fingerprints
                    .iter()
                    .find(|fingerprint| fingerprint.matches(&target_fingerprint))
                    .cloned()
                else {
                    break;
                };
                macos_delete_trusted_certificate(&fingerprint)?;
                removed += 1;
            }
            if removed >= MACOS_UNTRUST_REMOVAL_LIMIT
                && macos_trusted_jig_ca_fingerprints()?
                    .iter()
                    .any(|fingerprint| fingerprint.matches(&target_fingerprint))
            {
                bail!(
                    "Removed {removed} matching certificates, but more trusted copies remain. Run untrust again."
                );
            }
        }
        remove_trusted_ca_marker(store);
        Ok(json!({
            "ok": true,
            "platform": "macos",
            "removed": removed,
            "warning": macos_untrust_warning(removed),
        }))
    }

    #[cfg(target_os = "linux")]
    {
        let ca_exists = store.ca_path().exists();
        if ca_exists {
            ensure_jig_ca_certificate(store)?;
        }
        if !linux_command_available("trust") {
            bail!("Automatic certificate untrust requires the `trust` command.");
        }
        {
            let mut removed = 0usize;
            let current_trusted = if ca_exists {
                let current_der = first_certificate_der(&store.ca_path())?;
                linux_trust_anchors_contain_der(store, &current_der)?
            } else {
                false
            };
            let trusted_uris = linux_trusted_jig_ca_uris_result()?;
            let marker_authorizes_label_removal = if current_trusted {
                false
            } else if ca_exists {
                trusted_ca_marker_matches(store)?
            } else {
                trusted_ca_marker_owned_by_current_platform(store)?
            };
            if !ca_exists && !marker_authorizes_label_removal {
                bail!(
                    "CA certificate does not exist in {}, and no Jig-installed Linux trust marker was found.",
                    store.ca_path().display()
                );
            }
            if current_trusted || marker_authorizes_label_removal {
                if trusted_uris.is_empty() {
                    if current_trusted {
                        linux_remove_trust_anchor(command_path_arg(&store.ca_path()))?;
                        removed = 1;
                    }
                } else {
                    for uri in trusted_uris {
                        linux_remove_trust_anchor(OsString::from(uri))?;
                        removed += 1;
                    }
                }
            } else {
                bail!(
                    "No exact Jig CA trust anchor or Jig-installed trust marker was found. Refusing to remove label-matched Linux trust anchors."
                );
            }
            let bundle_update = if removed > 0 {
                linux_refresh_ca_bundles().context(
                    "trust anchor --remove succeeded, but refreshing system CA bundles failed",
                )?
            } else {
                json!({ "ok": true, "skipped": true })
            };
            remove_trusted_ca_marker(store);
            Ok(json!({
                "ok": true,
                "platform": "linux",
                "removed": removed,
                "system_bundle_update": bundle_update,
            }))
        }
    }
}

#[cfg(any(target_os = "macos", test))]
fn macos_untrust_warning(removed: usize) -> Option<&'static str> {
    if removed == 0 {
        Some("No trusted Jig Dev Proxy Local CA certificate was removed.")
    } else if removed >= MACOS_UNTRUST_REMOVAL_LIMIT {
        Some("Removed many matching certificates; run untrust again if more copies remain.")
    } else {
        None
    }
}

fn write_trusted_ca_marker(store: &StateStore) -> Result<()> {
    let record = TrustedCaRecord {
        version: TRUSTED_CA_VERSION,
        platform: std::env::consts::OS.to_string(),
        ca_hash: file_hash(&store.ca_path()).context("Failed to hash Jig proxy CA certificate")?,
        ca_fingerprint: trusted_ca_marker_fingerprint_for_current_platform(store)?,
        ca_sha256_fingerprint: trusted_ca_marker_sha256_fingerprint_for_current_platform(store)?,
    };
    write_owner_only_text(
        store.trusted_ca_path(),
        &serde_json::to_string_pretty(&record)?,
    )
}

fn ensure_jig_ca_certificate(store: &StateStore) -> Result<()> {
    let der = first_certificate_der(&store.ca_path())?;
    let (_, cert) = parse_x509_certificate(&der)
        .map_err(|error| anyhow!("Failed to parse Jig proxy CA certificate DER: {error}"))?;
    let basic_constraints = cert
        .basic_constraints()
        .map_err(|error| anyhow!("Failed to parse Jig proxy CA basic constraints: {error}"))?
        .context("Refusing to trust a certificate without CA basic constraints")?;
    if !basic_constraints.value.ca {
        bail!("Refusing to trust a certificate that is not a CA");
    }
    ensure_jig_ca_key_usages(&cert)?;
    let common_name = cert
        .subject()
        .iter_common_name()
        .next()
        .and_then(|common_name| common_name.as_str().ok());
    if common_name != Some(JIG_CA_COMMON_NAME) {
        bail!(
            "Refusing to trust CA certificate {} because it was not issued as a Jig Dev Proxy Local CA",
            store.ca_path().display()
        );
    }
    ensure_jig_ca_crypto_provenance(store)?;
    Ok(())
}

fn ensure_jig_ca_key_usages(cert: &x509_parser::certificate::X509Certificate<'_>) -> Result<()> {
    let key_usage = cert
        .key_usage()
        .map_err(|error| anyhow!("Failed to parse Jig proxy CA key usage: {error}"))?
        .context("Refusing to trust Jig proxy CA because it is missing key usage")?;
    let key_usage = key_usage.value;
    if !key_usage.key_cert_sign() || !key_usage.crl_sign() || !key_usage.digital_signature() {
        bail!("Refusing to trust Jig proxy CA because it is missing expected key usage");
    }
    Ok(())
}

fn ensure_jig_ca_crypto_provenance(store: &StateStore) -> Result<()> {
    let der = first_certificate_der(&store.ca_path())?;
    let (remaining, cert) = parse_x509_certificate(&der)
        .map_err(|error| anyhow!("Failed to parse Jig proxy CA certificate DER: {error}"))?;
    if !remaining.is_empty() {
        bail!("Refusing to trust Jig proxy CA certificate with trailing DER data");
    }
    if !cert.validity().is_valid() {
        bail!(
            "Refusing to trust Jig proxy CA certificate outside its validity period. Run `scripts/jig proxy cert generate --force` after untrusting the expired CA."
        );
    }
    if cert.subject() != cert.issuer() {
        bail!("Refusing to trust Jig proxy CA certificate that is not self-issued");
    }
    let public_key_algorithm = &cert.public_key().algorithm;
    if public_key_algorithm.algorithm != OID_KEY_TYPE_EC_PUBLIC_KEY {
        bail!("Refusing to trust Jig proxy CA certificate that is not an ECDSA P-256 CA");
    }
    let curve_oid = public_key_algorithm
        .parameters
        .as_ref()
        .and_then(|parameters| parameters.as_oid().ok());
    if curve_oid != Some(OID_EC_P256) {
        bail!("Refusing to trust Jig proxy CA certificate that is not an ECDSA P-256 CA");
    }
    if cert.signature_algorithm.algorithm != OID_SIG_ECDSA_WITH_SHA256 {
        bail!("Refusing to trust Jig proxy CA certificate without an ECDSA-SHA256 self-signature");
    }
    cert.verify_signature(None)
        .map_err(|error| anyhow!("Refusing to trust Jig proxy CA certificate: {error}"))?;
    Ok(())
}

fn ensure_jig_ca_private_key(store: &StateStore) -> Result<()> {
    if !private_key_matches_certificate(&store.ca_key_path(), &store.ca_path())? {
        bail!(
            "Refusing to trust Jig proxy CA because ca-key.pem does not match the CA certificate"
        );
    }
    Ok(())
}

fn private_key_matches_certificate(key_path: &Path, cert_path: &Path) -> Result<bool> {
    let key_pem = read_private_key(key_path)?;
    // rcgen::KeyPair does not zeroize its parsed key material on drop; the
    // zeroized PEM buffer keeps the readable copy scoped to this validation.
    let key_pair = KeyPair::from_pem(&key_pem)
        .with_context(|| format!("Failed to parse private key {}", key_path.display()))?;
    let der = first_certificate_der(cert_path)?;
    let (_, cert) = parse_x509_certificate(&der)
        .map_err(|error| anyhow!("Failed to parse certificate DER: {error}"))?;
    Ok(cert.public_key().subject_public_key.data.as_ref() == key_pair.public_key_raw())
}

fn remove_trusted_ca_marker(store: &StateStore) {
    match fs::remove_file(store.trusted_ca_path()) {
        Ok(()) => {}
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => eprintln!(
            "jig proxy could not remove trusted CA marker {}: {error}",
            store.trusted_ca_path().display()
        ),
    }
}

fn trusted_ca_marker_matches(store: &StateStore) -> Result<bool> {
    let Some(record) = read_trusted_ca_marker(store)? else {
        return Ok(false);
    };
    Ok(record.version == TRUSTED_CA_VERSION && Some(record.ca_hash) == file_hash(&store.ca_path()))
}

fn read_trusted_ca_marker(store: &StateStore) -> Result<Option<TrustedCaRecord>> {
    let Some(text) = file_ops::read_text_no_follow(&store.trusted_ca_path())? else {
        return Ok(None);
    };
    let record = serde_json::from_str::<TrustedCaRecord>(&text).with_context(|| {
        format!(
            "Failed to parse trusted CA marker {}",
            store.trusted_ca_path().display()
        )
    })?;
    Ok(Some(record))
}

#[cfg(target_os = "linux")]
fn trusted_ca_marker_owned_by_current_platform(store: &StateStore) -> Result<bool> {
    let Some(record) = read_trusted_ca_marker(store)? else {
        return Ok(false);
    };
    Ok(record.version == TRUSTED_CA_VERSION && record.platform == std::env::consts::OS)
}

fn trusted_ca_marker_fingerprint_for_current_platform(
    store: &StateStore,
) -> Result<Option<String>> {
    #[cfg(target_os = "macos")]
    {
        Ok(Some(ca_sha1_hex(&store.ca_path())?))
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = store;
        Ok(None)
    }
}

#[cfg(target_os = "macos")]
fn trusted_ca_marker_sha256_fingerprint_for_current_platform(
    store: &StateStore,
) -> Result<Option<String>> {
    Ok(Some(ca_sha256_hex(&store.ca_path())?))
}

#[cfg(not(target_os = "macos"))]
fn trusted_ca_marker_sha256_fingerprint_for_current_platform(
    store: &StateStore,
) -> Result<Option<String>> {
    let _ = store;
    Ok(None)
}

#[cfg(any(target_os = "macos", test))]
#[derive(Clone, Debug, Eq, PartialEq)]
struct CertificateFingerprints {
    sha1: String,
    sha256: String,
}

#[cfg(target_os = "macos")]
impl CertificateFingerprints {
    fn matches(&self, other: &Self) -> bool {
        self.sha1.eq_ignore_ascii_case(&other.sha1)
            && self.sha256.eq_ignore_ascii_case(&other.sha256)
    }
}

#[cfg(target_os = "macos")]
fn trusted_ca_marker_fingerprints(store: &StateStore) -> Result<Option<CertificateFingerprints>> {
    let Some(record) = read_trusted_ca_marker(store)? else {
        return Ok(None);
    };
    if record.version != TRUSTED_CA_VERSION || record.platform != "macos" {
        return Ok(None);
    }
    let Some(sha1) = record.ca_fingerprint else {
        return Ok(None);
    };
    let Some(sha256) = record.ca_sha256_fingerprint else {
        return Ok(None);
    };
    Ok(Some(CertificateFingerprints { sha1, sha256 }))
}

#[cfg(target_os = "macos")]
fn ca_sha1_hex(path: &Path) -> Result<String> {
    Ok(hex_upper(&Sha1::digest(first_certificate_der(path)?)))
}

#[cfg(target_os = "macos")]
fn ca_sha256_hex(path: &Path) -> Result<String> {
    Ok(hex_upper(&Sha256::digest(first_certificate_der(path)?)))
}

#[cfg(target_os = "macos")]
fn ca_fingerprints(path: &Path) -> Result<CertificateFingerprints> {
    Ok(CertificateFingerprints {
        sha1: ca_sha1_hex(path)?,
        sha256: ca_sha256_hex(path)?,
    })
}

fn first_certificate_der(path: &Path) -> Result<Vec<u8>> {
    let file = open_required_read_no_follow(path, MAX_CERT_PEM_BYTES)?;
    let mut reader = std::io::BufReader::new(file);
    let mut certs = rustls_pemfile::certs(&mut reader);
    let Some(cert) = certs.next() else {
        bail!(
            "CA certificate does not contain a PEM certificate: {}",
            path.display()
        );
    };
    let cert = cert.context("Failed to parse CA certificate PEM")?;
    if certs
        .next()
        .transpose()
        .context("Failed to parse CA certificate PEM")?
        .is_some()
    {
        bail!(
            "CA certificate contains more than one PEM certificate: {}",
            path.display()
        );
    }
    Ok(cert.as_ref().to_vec())
}

fn open_required_read_no_follow(path: &Path, max_bytes: u64) -> Result<fs::File> {
    open_optional_read_no_follow(path, max_bytes)?
        .with_context(|| format!("File does not exist: {}", path.display()))
}

fn open_optional_read_no_follow(path: &Path, max_bytes: u64) -> Result<Option<fs::File>> {
    let file = match file_ops::open_read_no_follow(path) {
        Ok(file) => file,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let metadata = file.metadata()?;
    let len = metadata.len();
    if len > max_bytes {
        bail!(
            "Refusing to read {} because it is {} bytes, above the {} byte limit",
            path.display(),
            len,
            max_bytes
        );
    }
    Ok(Some(file))
}

#[cfg(any(target_os = "macos", test))]
fn hex_upper(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

#[cfg(target_os = "macos")]
fn macos_trusted_ca_fingerprint_exists(fingerprint: &CertificateFingerprints) -> Result<bool> {
    Ok(macos_trusted_jig_ca_fingerprints()?
        .iter()
        .any(|hash| hash.matches(fingerprint)))
}

#[cfg(target_os = "macos")]
fn macos_trusted_jig_ca_fingerprints() -> Result<Vec<CertificateFingerprints>> {
    let Ok(keychain) = login_keychain_path() else {
        return Ok(Vec::new());
    };
    let mut command = macos_security_command();
    command
        .args([
            "find-certificate",
            "-a",
            "-Z",
            "-p",
            "-c",
            JIG_CA_COMMON_NAME,
        ])
        .arg(keychain);
    let output = command_output_with_timeout(&mut command, "security find-certificate")?;
    if output.status.success() {
        Ok(security_find_certificate_fingerprints(&output.stdout))
    } else {
        Ok(Vec::new())
    }
}

#[cfg(any(target_os = "macos", test))]
fn security_find_certificate_fingerprints(output: &[u8]) -> Vec<CertificateFingerprints> {
    let text = String::from_utf8_lossy(output);
    let mut fingerprints = Vec::new();
    let mut pending_sha1 = None;
    let mut pem = String::new();
    let mut in_pem = false;
    for line in text.lines() {
        if let Some(hash) = line.trim().strip_prefix("SHA-1 hash:") {
            let hash = hash.trim().to_ascii_uppercase();
            pending_sha1 = sha1_fingerprint_is_valid(&hash).then_some(hash);
            continue;
        }
        if line == "-----BEGIN CERTIFICATE-----" {
            pem.clear();
            pem.push_str(line);
            pem.push('\n');
            in_pem = true;
            continue;
        }
        if in_pem {
            pem.push_str(line);
            pem.push('\n');
            if line == "-----END CERTIFICATE-----" {
                if let Some(sha1) = pending_sha1.take() {
                    if let Some(sha256) = pem_sha256_hex(&pem) {
                        fingerprints.push(CertificateFingerprints { sha1, sha256 });
                    }
                }
                in_pem = false;
            }
        }
    }
    fingerprints
}

#[cfg(any(target_os = "macos", test))]
fn pem_sha256_hex(pem: &str) -> Option<String> {
    let mut reader = std::io::BufReader::new(pem.as_bytes());
    let cert = rustls_pemfile::certs(&mut reader).next()?.ok()?;
    Some(hex_upper(&Sha256::digest(cert.as_ref())))
}

#[cfg(target_os = "macos")]
fn macos_delete_trusted_certificate(fingerprint: &CertificateFingerprints) -> Result<()> {
    if !sha1_fingerprint_is_valid(&fingerprint.sha1) {
        bail!("Refusing to pass invalid SHA-1 certificate fingerprint to security");
    }
    let keychain = login_keychain_path()?;
    let mut command = macos_security_command();
    // The candidate was selected by a paired SHA-256 PEM digest above; the
    // `security delete-certificate -Z` interface itself accepts the SHA-1 hash.
    command
        .args(["delete-certificate", "-Z", &fingerprint.sha1])
        .arg(&keychain);
    let output = command_output_with_timeout(&mut command, "security delete-certificate")?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    if stderr.contains("could not be found") || stderr.contains("not found") {
        bail!(
            "security reported a matching Jig CA certificate but could not delete it; run `scripts/jig proxy cert untrust --accept-trust-scope` again."
        );
    }
    bail!("security delete-certificate failed: {}", stderr.trim())
}

#[cfg(any(target_os = "macos", test))]
fn sha1_fingerprint_is_valid(fingerprint: &str) -> bool {
    fingerprint.len() == 40
        && fingerprint
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_lowercase())
}

fn trust_check(store: &StateStore) -> Value {
    #[cfg(target_os = "macos")]
    {
        let mut error = None::<String>;
        let fingerprints = if store.ca_path().exists() {
            match ca_fingerprints(&store.ca_path()) {
                Ok(fingerprints) => Some(fingerprints),
                Err(err) => {
                    error = Some(err.to_string());
                    None
                }
            }
        } else {
            None
        };
        let trusted = if let Some(fingerprints) = fingerprints.as_ref() {
            match macos_trusted_ca_fingerprint_exists(fingerprints) {
                Ok(trusted) => Some(trusted),
                Err(err) => {
                    error = Some(err.to_string());
                    None
                }
            }
        } else if store.ca_path().exists() {
            None
        } else {
            Some(false)
        };
        json!({
            "platform": "macos",
            "trusted": trusted,
            "fingerprint_sha256": fingerprints.as_ref().map(|fingerprint| &fingerprint.sha256),
            "fingerprint_sha1": fingerprints.as_ref().map(|fingerprint| &fingerprint.sha1),
            "error": error,
        })
    }

    #[cfg(target_os = "linux")]
    {
        let has_trust = linux_command_available("trust");
        let mut error = None::<String>;
        let trusted = if has_trust {
            if store.ca_path().exists() {
                match linux_current_jig_ca_is_trusted(store) {
                    Ok(trusted) => Some(trusted),
                    Err(err) => {
                        error = Some(err.to_string());
                        None
                    }
                }
            } else {
                match linux_trusted_jig_ca_uris_result() {
                    Ok(uris) => Some(!uris.is_empty()),
                    Err(err) => {
                        error = Some(err.to_string());
                        None
                    }
                }
            }
        } else {
            None
        };
        json!({ "platform": "linux", "trusted": trusted, "trust_command": has_trust, "error": error })
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = store;
        json!({ "platform": std::env::consts::OS, "trusted": null })
    }
}

#[cfg(target_os = "linux")]
fn linux_refresh_ca_bundles() -> Result<Value> {
    if linux_command_available("update-ca-trust") {
        let mut command = linux_system_command("update-ca-trust")?;
        command.arg("extract");
        let status = command_status_with_timeout(&mut command, "update-ca-trust extract")?;
        if !status.success() {
            bail!(
                "update-ca-trust extract failed with status {status}. Run with the privileges required by your distribution or refresh system CA bundles manually."
            );
        }
        return Ok(json!({
            "ok": true,
            "command": "update-ca-trust extract",
            "status": status.code(),
        }));
    }
    if linux_command_available("update-ca-certificates") {
        let mut command = linux_system_command("update-ca-certificates")?;
        let status = command_status_with_timeout(&mut command, "update-ca-certificates")?;
        if !status.success() {
            bail!(
                "update-ca-certificates failed with status {status}. Run with the privileges required by your distribution or refresh system CA bundles manually."
            );
        }
        return Ok(json!({
            "ok": true,
            "command": "update-ca-certificates",
            "status": status.code(),
        }));
    }
    bail!(
        "No supported system CA bundle refresh command found. Install update-ca-trust/update-ca-certificates, run with privileges when required, or refresh system CA bundles manually."
    )
}

#[cfg(target_os = "linux")]
fn linux_command_available(program: &str) -> bool {
    linux_system_tool(program).is_some()
}

#[cfg(target_os = "linux")]
fn linux_system_command(program: &str) -> Result<Command> {
    let path = linux_system_tool(program)
        .with_context(|| format!("Could not find supported system command `{program}`"))?;
    let mut command = Command::new(path);
    command.env_clear();
    Ok(command)
}

#[cfg(target_os = "linux")]
fn linux_system_tool(program: &str) -> Option<PathBuf> {
    linux_system_tool_candidates(program)
        .iter()
        .map(PathBuf::from)
        .find(|path| executable_file(path))
}

#[cfg(target_os = "linux")]
fn linux_system_tool_candidates(program: &str) -> &'static [&'static str] {
    match program {
        "trust" => &["/usr/bin/trust", "/bin/trust"],
        "update-ca-trust" => &["/usr/bin/update-ca-trust", "/usr/sbin/update-ca-trust"],
        "update-ca-certificates" => &[
            "/usr/sbin/update-ca-certificates",
            "/usr/bin/update-ca-certificates",
        ],
        _ => &[],
    }
}

#[cfg(target_os = "linux")]
fn executable_file(path: &Path) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
}

#[cfg(target_os = "linux")]
fn linux_trusted_jig_ca_uris() -> Vec<String> {
    linux_trusted_jig_ca_uris_result().unwrap_or_default()
}

#[cfg(target_os = "linux")]
fn linux_trusted_jig_ca_uris_result() -> Result<Vec<String>> {
    if !linux_command_available("trust") {
        return Ok(Vec::new());
    }
    let mut command = linux_system_command("trust")?;
    command.args(["list", "--filter=ca-anchors"]);
    let output = command_output_with_timeout(&mut command, "trust list")?;
    if !output.status.success() {
        bail!(
            "trust list --filter=ca-anchors failed with status {}",
            output.status
        );
    }
    Ok(trust_list_jig_ca_uris(&output.stdout))
}

#[cfg(target_os = "linux")]
fn linux_remove_trust_anchor(anchor: OsString) -> Result<()> {
    let mut command = linux_system_command("trust")?;
    command.arg("anchor").arg("--remove").arg(anchor);
    let status = command_status_with_timeout(&mut command, "trust anchor --remove")?;
    if !status.success() {
        bail!("trust anchor --remove failed with status {status}");
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn linux_current_jig_ca_is_trusted(store: &StateStore) -> Result<bool> {
    if !linux_command_available("trust") {
        return Ok(false);
    }
    let current_der = first_certificate_der(&store.ca_path())?;
    if linux_trust_anchors_contain_der(store, &current_der)? {
        return Ok(true);
    }
    // Older p11-kit deployments may not support the extract format consistently.
    // Fall back to Jig's owned CA label so forced regeneration still refuses
    // when a prior Jig root might be trusted.
    Ok(!linux_trusted_jig_ca_uris_result()?.is_empty())
}

#[cfg(target_os = "linux")]
fn linux_trust_anchors_contain_der(store: &StateStore, expected_der: &[u8]) -> Result<bool> {
    let tmp_dir = file_ops::temp_path(&store.root().join("trusted-anchors"), "jig-proxy-cert");
    fs::create_dir(&tmp_dir)?;
    #[cfg(unix)]
    fs::set_permissions(&tmp_dir, fs::Permissions::from_mode(0o700))?;
    let tmp = tmp_dir.join("anchors.pem");
    let mut command = match linux_system_command("trust") {
        Ok(command) => command,
        Err(error) => {
            let _ = fs::remove_dir(&tmp_dir);
            return Err(error);
        }
    };
    command
        .args([
            "extract",
            "--overwrite",
            "--format=pem-bundle",
            "--filter=ca-anchors",
        ])
        .arg(command_path_arg(&tmp));
    let status = match command_status_with_timeout(&mut command, "trust extract") {
        Ok(status) => status,
        Err(error) => {
            let _ = fs::remove_file(&tmp);
            let _ = fs::remove_dir(&tmp_dir);
            return Err(error);
        }
    };
    if !status.success() {
        let _ = fs::remove_file(&tmp);
        let _ = fs::remove_dir(&tmp_dir);
        bail!("trust extract failed with status {status}; refusing to assume no Jig CA is trusted");
    }
    let result = pem_bundle_contains_der(&tmp, expected_der);
    let _ = fs::remove_file(&tmp);
    let _ = fs::remove_dir(&tmp_dir);
    result
}

#[cfg(target_os = "linux")]
fn pem_bundle_contains_der(path: &Path, expected_der: &[u8]) -> Result<bool> {
    let file = open_required_read_no_follow(path, MAX_TRUST_BUNDLE_PEM_BYTES)?;
    let mut reader = std::io::BufReader::new(file);
    for cert in rustls_pemfile::certs(&mut reader) {
        let cert = cert.context("Failed to parse trust anchor PEM bundle")?;
        if cert.as_ref() == expected_der {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(any(target_os = "linux", test))]
fn trust_list_jig_ca_uris(output: &[u8]) -> Vec<String> {
    let mut uris = Vec::new();
    let mut current_uri = None::<String>;
    let mut current_label_matches = false;
    for line in String::from_utf8_lossy(output).lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("pkcs11:") {
            if current_label_matches {
                if let Some(uri) = current_uri.take() {
                    uris.push(uri);
                }
            }
            current_uri = Some(trimmed.to_string());
            current_label_matches = false;
            continue;
        }
        if trimmed
            .strip_prefix("label:")
            .map(str::trim)
            .map(|label| label.trim_matches('"'))
            .is_some_and(|label| label == JIG_CA_COMMON_NAME)
        {
            current_label_matches = true;
        }
    }
    if current_label_matches {
        if let Some(uri) = current_uri {
            uris.push(uri);
        }
    }
    uris
}

#[cfg(target_os = "macos")]
fn login_keychain_path() -> Result<PathBuf> {
    let home = dirs::home_dir().context("Could not resolve home directory for login keychain")?;
    Ok(home.join("Library/Keychains/login.keychain-db"))
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;
    use crate::types::{Route, RouteMode};

    #[test]
    fn ca_provenance_rejects_non_jig_common_name() {
        let temp = tempdir().unwrap();
        let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
        let mut params = ca_params(&ProxySettings::default()).unwrap();
        params.distinguished_name = DistinguishedName::new();
        params
            .distinguished_name
            .push(DnType::CommonName, "Not Jig");
        let key = KeyPair::generate().unwrap();
        let cert = params.self_signed(&key).unwrap();
        fs::write(store.ca_path(), cert.pem()).unwrap();
        fs::write(store.ca_key_path(), key.serialize_pem()).unwrap();

        let error = ensure_jig_ca_certificate(&store).unwrap_err().to_string();

        assert!(error.contains("was not issued as a Jig Dev Proxy Local CA"));
    }

    #[test]
    fn ca_provenance_rejects_missing_key_usage() {
        let temp = tempdir().unwrap();
        let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
        let mut params = ca_params(&ProxySettings::default()).unwrap();
        params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
        let key = KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256).unwrap();
        let cert = params.self_signed(&key).unwrap();
        fs::write(store.ca_path(), cert.pem()).unwrap();
        fs::write(store.ca_key_path(), key.serialize_pem()).unwrap();

        let error = ensure_jig_ca_certificate(&store).unwrap_err().to_string();

        assert!(error.contains("missing expected key usage"));
    }

    #[test]
    fn ca_provenance_rejects_expired_ca_certificate() {
        let temp = tempdir().unwrap();
        let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
        let mut params = ca_params(&ProxySettings::default()).unwrap();
        params.not_before = OffsetDateTime::now_utc() - TimeDuration::days(10);
        params.not_after = OffsetDateTime::now_utc() - TimeDuration::days(1);
        let key = KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256).unwrap();
        let cert = params.self_signed(&key).unwrap();
        fs::write(store.ca_path(), cert.pem()).unwrap();
        fs::write(store.ca_key_path(), key.serialize_pem()).unwrap();

        let error = ensure_jig_ca_certificate(&store).unwrap_err().to_string();

        assert!(error.contains("outside its validity period"));
    }

    #[cfg(unix)]
    #[test]
    fn ca_provenance_rejects_multiple_pem_certificates() {
        let temp = tempdir().unwrap();
        let settings = ProxySettings {
            state_dir: Some(temp.path().to_path_buf()),
            ..ProxySettings::default()
        };
        generate(&settings, false).unwrap();
        let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
        let pem = fs::read_to_string(store.ca_path()).unwrap();
        fs::write(store.ca_path(), format!("{pem}\n{pem}")).unwrap();

        let error = ensure_jig_ca_certificate(&store).unwrap_err().to_string();

        assert!(error.contains("more than one PEM certificate"));
    }

    #[cfg(unix)]
    #[test]
    fn trust_rejects_ca_key_mismatch() {
        let temp = tempdir().unwrap();
        let settings = ProxySettings {
            state_dir: Some(temp.path().to_path_buf()),
            ..ProxySettings::default()
        };
        generate(&settings, false).unwrap();
        let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
        let other_key = KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256).unwrap();
        fs::write(store.ca_key_path(), other_key.serialize_pem()).unwrap();

        let error = trust(&settings, false).unwrap_err().to_string();

        assert!(error.contains("does not match the CA certificate"));
    }

    #[cfg(unix)]
    #[test]
    fn trust_requires_accept_trust_scope() {
        let temp = tempdir().unwrap();
        let settings = ProxySettings {
            state_dir: Some(temp.path().to_path_buf()),
            ..ProxySettings::default()
        };
        generate(&settings, false).unwrap();

        let error = trust(&settings, false).unwrap_err().to_string();

        assert!(error.contains("--accept-trust-scope"));
    }

    #[test]
    fn untrust_requires_accept_trust_scope() {
        let temp = tempdir().unwrap();
        let settings = ProxySettings {
            state_dir: Some(temp.path().to_path_buf()),
            ..ProxySettings::default()
        };

        let error = untrust(&settings, false).unwrap_err().to_string();

        assert!(error.contains("--accept-trust-scope"));
    }

    #[cfg(not(windows))]
    #[test]
    fn security_fingerprint_parser_pairs_sha1_with_pem_sha256() {
        let temp = tempdir().unwrap();
        let settings = ProxySettings {
            state_dir: Some(temp.path().to_path_buf()),
            ..ProxySettings::default()
        };
        generate(&settings, false).unwrap();
        let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
        let pem = fs::read_to_string(store.ca_path()).unwrap();
        let expected_sha256 = pem_sha256_hex(&pem).unwrap();
        let output = format!("SHA-1 hash: 00112233445566778899AABBCCDDEEFF00112233\n{pem}");

        assert_eq!(
            security_find_certificate_fingerprints(output.as_bytes()),
            vec![CertificateFingerprints {
                sha1: "00112233445566778899AABBCCDDEEFF00112233".to_string(),
                sha256: expected_sha256,
            }]
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn security_fingerprint_parser_rejects_malformed_sha1() {
        let temp = tempdir().unwrap();
        let settings = ProxySettings {
            state_dir: Some(temp.path().to_path_buf()),
            ..ProxySettings::default()
        };
        generate(&settings, false).unwrap();
        let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
        let pem = fs::read_to_string(store.ca_path()).unwrap();
        let output = format!("SHA-1 hash: not-a-fingerprint\n{pem}");

        assert!(security_find_certificate_fingerprints(output.as_bytes()).is_empty());
    }

    #[cfg(windows)]
    #[test]
    fn generate_rejects_windows_private_key_writes() {
        let temp = tempdir().unwrap();
        let settings = ProxySettings {
            state_dir: Some(temp.path().to_path_buf()),
            ..ProxySettings::default()
        };

        let error = generate(&settings, false).unwrap_err().to_string();

        assert!(error.contains("not supported on Windows"));
    }

    #[test]
    fn certificate_hosts_include_existing_routes_and_new_hosts() {
        let temp = tempdir().unwrap();
        let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
        store
            .add_route(Route {
                hostname: "web.demo.localhost".into(),
                target_host: "127.0.0.1".into(),
                target_port: 4000,
                owner_pid: None,
                owner_start_token: None,
                mode: RouteMode::Alias,
                created_at_ms: crate::state::now_ms(),
            })
            .unwrap();
        let settings = ProxySettings {
            state_dir: Some(temp.path().to_path_buf()),
            additional_dns_names: vec!["api.demo.localhost".into()],
            ..ProxySettings::default()
        };

        let hosts = certificate_hosts(&settings, &store, &["admin.demo.localhost".into()]).unwrap();

        assert!(hosts.contains(&"localhost".to_string()));
        assert!(!hosts.contains(&"*.localhost".to_string()));
        assert!(hosts.contains(&"api.demo.localhost".to_string()));
        assert!(hosts.contains(&"web.demo.localhost".to_string()));
        assert!(hosts.contains(&"admin.demo.localhost".to_string()));
    }

    #[test]
    fn certificate_hosts_do_not_add_bare_wildcard_for_custom_tld() {
        let temp = tempdir().unwrap();
        let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
        let settings = ProxySettings {
            state_dir: Some(temp.path().to_path_buf()),
            tld: "corp.example".into(),
            additional_dns_names: vec!["*.demo.corp.example".into()],
            ..ProxySettings::default()
        };

        let hosts = certificate_hosts(&settings, &store, &[]).unwrap();

        assert!(hosts.contains(&"*.demo.corp.example".to_string()));
        assert!(!hosts.contains(&"corp.example".to_string()));
        assert!(!hosts.contains(&"*.corp.example".to_string()));
    }

    #[test]
    fn certificate_hosts_do_not_add_bare_local_wildcard_in_lan_mode() {
        let temp = tempdir().unwrap();
        let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
        let settings = ProxySettings {
            state_dir: Some(temp.path().to_path_buf()),
            tld: "local".into(),
            lan: true,
            additional_dns_names: vec!["*.demo.local".into()],
            ..ProxySettings::default()
        };

        let hosts = certificate_hosts(&settings, &store, &[]).unwrap();

        assert!(hosts.contains(&"*.demo.local".to_string()));
        assert!(!hosts.contains(&"local".to_string()));
        assert!(!hosts.contains(&"*.local".to_string()));
    }

    #[test]
    fn certificate_hosts_reject_invalid_additional_dns_name() {
        let temp = tempdir().unwrap();
        let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
        let settings = ProxySettings {
            state_dir: Some(temp.path().to_path_buf()),
            additional_dns_names: vec!["bad,name.localhost".into()],
            ..ProxySettings::default()
        };

        let error = certificate_hosts(&settings, &store, &[])
            .unwrap_err()
            .to_string();

        assert!(error.contains("invalid hostname"));
    }

    #[test]
    fn certificate_hosts_normalize_dns_names_to_lowercase() {
        let temp = tempdir().unwrap();
        let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
        let settings = ProxySettings {
            state_dir: Some(temp.path().to_path_buf()),
            tld: "LOCALHOST".into(),
            additional_dns_names: vec!["API.DEMO.LOCALHOST".into()],
            ..ProxySettings::default()
        };

        let hosts = certificate_hosts(&settings, &store, &[]).unwrap();

        assert!(hosts.contains(&"api.demo.localhost".to_string()));
        assert!(hosts.contains(&"localhost".to_string()));
        assert!(!hosts.contains(&"*.localhost".to_string()));
    }

    #[test]
    fn certificate_hosts_reject_excessive_sans() {
        let temp = tempdir().unwrap();
        let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
        let settings = ProxySettings {
            state_dir: Some(temp.path().to_path_buf()),
            additional_dns_names: (0..MAX_CERTIFICATE_HOSTS)
                .map(|index| format!("host-{index}.localhost"))
                .collect(),
            ..ProxySettings::default()
        };

        let error = certificate_hosts(&settings, &store, &[])
            .unwrap_err()
            .to_string();

        assert!(error.contains("SAN entries"));
    }

    #[test]
    fn cert_temp_names_require_generated_suffix() {
        assert!(is_cert_temp_name("ca.pem.123.456.0.tmp"));
        assert!(is_cert_temp_name("leaf-hosts.json.123.456.0.tmp"));
        assert!(is_cert_temp_name("trusted-ca.json.123.456.0.tmp"));
        assert!(is_cert_temp_name("trusted-anchors.pem.123.456.0.tmp"));
        assert!(!is_cert_temp_name("ca.pem.attacker.tmp"));
        assert!(!is_cert_temp_name("leaf.pem.123.tmp"));
    }

    #[test]
    fn linux_trust_list_parser_finds_jig_ca_uris() {
        let output = br#"
pkcs11:id=%01;type=cert
    type: certificate
    label: Jig Dev Proxy Local CA
pkcs11:id=%02;type=cert
    type: certificate
    label: Other CA
pkcs11:id=%03;type=cert
    type: certificate
    label: Jig Dev Proxy Local CA
"#;

        assert_eq!(
            trust_list_jig_ca_uris(output),
            vec![
                "pkcs11:id=%01;type=cert".to_string(),
                "pkcs11:id=%03;type=cert".to_string()
            ]
        );
        assert!(trust_list_jig_ca_uris(b"label: Other CA\n").is_empty());
    }

    #[test]
    fn macos_untrust_warning_only_reports_limit() {
        assert!(macos_untrust_warning(0).is_some());
        assert_eq!(macos_untrust_warning(10), None);
        assert!(macos_untrust_warning(MACOS_UNTRUST_REMOVAL_LIMIT).is_some());
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn command_path_arg_disambiguates_dash_prefixed_relative_paths() {
        assert_eq!(
            command_path_arg(Path::new("-state/ca.pem")),
            Path::new(".").join("-state/ca.pem").into_os_string()
        );
        assert_eq!(
            command_path_arg(Path::new("state/ca.pem")),
            Path::new("state/ca.pem").as_os_str().to_owned()
        );
    }

    #[cfg(any(
        target_os = "linux",
        target_os = "macos",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd"
    ))]
    #[test]
    fn certificate_hosts_prune_dead_process_routes() {
        let temp = tempdir().unwrap();
        let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
        store
            .add_route(Route {
                hostname: "stale.demo.localhost".into(),
                target_host: "127.0.0.1".into(),
                target_port: 4000,
                owner_pid: Some(999_999),
                owner_start_token: Some("dead-process".into()),
                mode: RouteMode::Process,
                created_at_ms: crate::state::now_ms(),
            })
            .unwrap();
        let settings = ProxySettings {
            state_dir: Some(temp.path().to_path_buf()),
            ..ProxySettings::default()
        };

        let hosts = certificate_hosts(&settings, &store, &[]).unwrap();

        assert!(!hosts.contains(&"stale.demo.localhost".to_string()));
    }

    #[cfg(unix)]
    #[test]
    fn generated_private_keys_are_owner_only() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempdir().unwrap();
        let settings = ProxySettings {
            state_dir: Some(temp.path().to_path_buf()),
            ..ProxySettings::default()
        };

        generate(&settings, false).unwrap();
        let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
        let ca_mode = fs::metadata(store.ca_key_path())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        let leaf_mode = fs::metadata(store.leaf_key_path())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;

        assert_eq!(ca_mode, 0o600);
        assert_eq!(leaf_mode, 0o600);
    }

    #[cfg(unix)]
    #[test]
    fn private_key_reads_reject_group_or_world_readable_files() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempdir().unwrap();
        let key = temp.path().join("key.pem");
        fs::write(&key, "not a key").unwrap();
        fs::set_permissions(&key, fs::Permissions::from_mode(0o644)).unwrap();

        let error = read_optional_private_key(&key).unwrap_err().to_string();

        assert!(error.contains("permissions 644"));
    }

    #[cfg(unix)]
    #[test]
    fn private_key_reads_reject_symlink_files() {
        use std::os::unix::fs::symlink;

        let temp = tempdir().unwrap();
        let key = temp.path().join("key.pem");
        let target = temp.path().join("target.pem");
        fs::write(&target, "not a key").unwrap();
        symlink(&target, &key).unwrap();

        let error = read_optional_private_key(&key).unwrap_err().to_string();

        assert!(
            error.contains("Too many levels of symbolic links")
                || error.contains("symbolic link")
                || error.contains("os error 40")
        );
    }

    #[cfg(unix)]
    #[test]
    fn ensure_keeps_existing_leaf_when_hosts_are_unchanged() {
        let temp = tempdir().unwrap();
        let settings = ProxySettings {
            state_dir: Some(temp.path().to_path_buf()),
            additional_dns_names: vec!["api.demo.localhost".into()],
            ..ProxySettings::default()
        };

        generate(&settings, false).unwrap();
        let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
        let first_key = fs::read_to_string(store.leaf_key_path()).unwrap();
        let first_cert = fs::read_to_string(store.leaf_path()).unwrap();

        ensure_for_hosts(&settings, &[]).unwrap();

        assert_eq!(
            fs::read_to_string(store.leaf_key_path()).unwrap(),
            first_key
        );
        assert_eq!(fs::read_to_string(store.leaf_path()).unwrap(), first_cert);
        assert!(store.leaf_hosts_path().exists());
    }

    #[cfg(unix)]
    #[test]
    fn ensure_keeps_existing_leaf_when_hosts_are_a_superset() {
        let temp = tempdir().unwrap();
        let prepared_settings = ProxySettings {
            state_dir: Some(temp.path().to_path_buf()),
            additional_dns_names: vec!["web.demo.localhost".into()],
            ..ProxySettings::default()
        };

        generate(&prepared_settings, false).unwrap();
        let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
        let first_cert = fs::read_to_string(store.leaf_path()).unwrap();
        let contextless_settings = ProxySettings {
            state_dir: Some(temp.path().to_path_buf()),
            ..ProxySettings::default()
        };

        ensure(&contextless_settings).unwrap();

        assert_eq!(fs::read_to_string(store.leaf_path()).unwrap(), first_cert);
        assert!(
            fs::read_to_string(store.leaf_hosts_path())
                .unwrap()
                .contains("web.demo.localhost")
        );
    }

    #[cfg(unix)]
    #[test]
    fn generated_leaf_verifies_against_generated_ca() {
        let temp = tempdir().unwrap();
        let settings = ProxySettings {
            state_dir: Some(temp.path().to_path_buf()),
            additional_dns_names: vec!["web.demo.localhost".into()],
            ..ProxySettings::default()
        };

        generate(&settings, false).unwrap();
        let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
        let ca_der = first_certificate_der(&store.ca_path()).unwrap();
        let leaf_der = first_certificate_der(&store.leaf_path()).unwrap();
        let (_, ca) = parse_x509_certificate(&ca_der).unwrap();
        let (_, leaf) = parse_x509_certificate(&leaf_der).unwrap();

        let constraints = ca.name_constraints().unwrap().unwrap().value;
        assert!(constraints.permitted_subtrees.is_some());
        assert!(constraints.excluded_subtrees.is_none());
        leaf.verify_signature(Some(ca.public_key())).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn ca_constraints_must_cover_additional_dns_and_ip_settings() {
        let temp = tempdir().unwrap();
        let base_settings = ProxySettings {
            state_dir: Some(temp.path().to_path_buf()),
            ..ProxySettings::default()
        };
        generate(&base_settings, false).unwrap();
        let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
        let same_tld_settings = ProxySettings {
            state_dir: Some(temp.path().to_path_buf()),
            additional_dns_names: vec!["*.demo.localhost".into()],
            ..ProxySettings::default()
        };
        assert!(ca_name_constraints_cover_settings(&store.ca_path(), &same_tld_settings).unwrap());

        let extended_settings = ProxySettings {
            state_dir: Some(temp.path().to_path_buf()),
            additional_dns_names: vec!["*.other.local".into(), "192.0.2.10".into()],
            ..ProxySettings::default()
        };

        assert!(!ca_name_constraints_cover_settings(&store.ca_path(), &extended_settings).unwrap());

        generate(&extended_settings, true).unwrap();
        assert!(ca_name_constraints_cover_settings(&store.ca_path(), &extended_settings).unwrap());
    }

    #[test]
    fn ca_constraints_reject_public_additional_dns_name() {
        let settings = ProxySettings {
            additional_dns_names: vec!["com".into()],
            ..ProxySettings::default()
        };

        let error = ca_dns_name_constraints(&settings).unwrap_err().to_string();

        assert!(error.contains("configured Jig development DNS name"));
    }

    #[test]
    fn ca_constraints_do_not_permit_bare_non_localhost_tld() {
        let settings = ProxySettings {
            tld: "internal".into(),
            additional_dns_names: vec!["*.demo.internal".into()],
            ..ProxySettings::default()
        };

        let constraints = ca_dns_name_constraints(&settings).unwrap();

        assert!(constraints.contains(&"demo.internal".to_string()));
        assert!(!constraints.contains(&"internal".to_string()));
    }

    #[test]
    fn empty_dns_constraint_does_not_cover_hosts() {
        assert!(!dns_constraint_covers("", "web.demo.localhost"));
    }

    #[cfg(unix)]
    #[test]
    fn write_leaf_rejects_hosts_outside_ca_name_constraints() {
        let temp = tempdir().unwrap();
        let settings = ProxySettings {
            state_dir: Some(temp.path().to_path_buf()),
            ..ProxySettings::default()
        };
        generate(&settings, false).unwrap();
        let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();

        let error = write_leaf(&store, &["example.com".into()])
            .unwrap_err()
            .to_string();

        assert!(error.contains("outside the CA name constraints"));
    }

    #[cfg(unix)]
    #[test]
    fn ensure_reuses_existing_leaf_key_when_hosts_change() {
        let temp = tempdir().unwrap();
        let settings = ProxySettings {
            state_dir: Some(temp.path().to_path_buf()),
            ..ProxySettings::default()
        };

        generate(&settings, false).unwrap();
        let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
        let first_key = fs::read_to_string(store.leaf_key_path()).unwrap();
        let first_cert = fs::read_to_string(store.leaf_path()).unwrap();

        ensure_for_hosts(&settings, &["api.demo.localhost".into()]).unwrap();

        assert_eq!(
            fs::read_to_string(store.leaf_key_path()).unwrap(),
            first_key
        );
        assert_ne!(fs::read_to_string(store.leaf_path()).unwrap(), first_cert);
        assert!(
            fs::read_to_string(store.leaf_hosts_path())
                .unwrap()
                .contains("api.demo.localhost")
        );
    }

    #[cfg(unix)]
    #[test]
    fn leaf_sidecar_records_full_sha256_hashes() {
        let temp = tempdir().unwrap();
        let settings = ProxySettings {
            state_dir: Some(temp.path().to_path_buf()),
            https: true,
            ..ProxySettings::default()
        };

        generate(&settings, false).unwrap();
        let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
        let text = fs::read_to_string(store.leaf_hosts_path()).unwrap();
        let record: LeafHostsRecord = serde_json::from_str(&text).unwrap();

        assert_eq!(record.ca_hash.len(), 64);
        assert_eq!(record.certificate_hash.len(), 64);
        assert!(
            record
                .ca_hash
                .chars()
                .all(|value| value.is_ascii_hexdigit())
        );
        assert!(
            record
                .certificate_hash
                .chars()
                .all(|value| value.is_ascii_hexdigit())
        );
        assert_eq!(Some(record.ca_hash), file_hash(&store.ca_path()));
        assert_eq!(Some(record.certificate_hash), file_hash(&store.leaf_path()));
    }

    #[cfg(unix)]
    #[test]
    fn ensure_regenerates_when_leaf_sidecar_hash_is_stale() {
        let temp = tempdir().unwrap();
        let settings = ProxySettings {
            state_dir: Some(temp.path().to_path_buf()),
            https: true,
            ..ProxySettings::default()
        };
        generate(&settings, false).unwrap();
        let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
        let first_cert = fs::read_to_string(store.leaf_path()).unwrap();
        let text = fs::read_to_string(store.leaf_hosts_path()).unwrap();
        let mut record: LeafHostsRecord = serde_json::from_str(&text).unwrap();
        record.certificate_hash = "stale-certificate-hash".into();
        fs::write(
            store.leaf_hosts_path(),
            serde_json::to_string_pretty(&record).unwrap(),
        )
        .unwrap();

        ensure_for_hosts(&settings, &[]).unwrap();

        assert_ne!(fs::read_to_string(store.leaf_path()).unwrap(), first_cert);
    }

    #[cfg(unix)]
    #[test]
    fn ensure_regenerates_when_leaf_sidecar_ca_hash_is_stale() {
        let temp = tempdir().unwrap();
        let settings = ProxySettings {
            state_dir: Some(temp.path().to_path_buf()),
            https: true,
            ..ProxySettings::default()
        };
        generate(&settings, false).unwrap();
        let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
        let first_cert = fs::read_to_string(store.leaf_path()).unwrap();
        let text = fs::read_to_string(store.leaf_hosts_path()).unwrap();
        let mut record: LeafHostsRecord = serde_json::from_str(&text).unwrap();
        record.ca_hash = "stale-ca-hash".into();
        fs::write(
            store.leaf_hosts_path(),
            serde_json::to_string_pretty(&record).unwrap(),
        )
        .unwrap();

        ensure_for_hosts(&settings, &[]).unwrap();

        assert_ne!(fs::read_to_string(store.leaf_path()).unwrap(), first_cert);
    }

    #[cfg(unix)]
    #[test]
    fn ensure_regenerates_expired_leaf_even_when_sidecar_matches() {
        let temp = tempdir().unwrap();
        let settings = ProxySettings {
            state_dir: Some(temp.path().to_path_buf()),
            https: true,
            ..ProxySettings::default()
        };
        generate(&settings, false).unwrap();
        let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
        let hosts = certificate_hosts(&settings, &store, &[]).unwrap();

        let ca_cert_pem = fs::read_to_string(store.ca_path()).unwrap();
        let ca_key_pem = read_private_key(&store.ca_key_path()).unwrap();
        let ca_key = KeyPair::from_pem(&ca_key_pem).unwrap();
        let ca = Issuer::from_ca_cert_pem(&ca_cert_pem, ca_key).unwrap();
        let leaf_key_pem = read_private_key(&store.leaf_key_path()).unwrap();
        let leaf_key = KeyPair::from_pem(&leaf_key_pem).unwrap();
        let mut leaf_params = CertificateParams::new(hosts.clone()).unwrap();
        leaf_params.not_before = OffsetDateTime::now_utc() - TimeDuration::days(3);
        leaf_params.not_after = OffsetDateTime::now_utc() - TimeDuration::days(2);
        leaf_params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
        let expired = leaf_params.signed_by(&leaf_key, &ca).unwrap();
        write_public_pem(store.leaf_path(), &expired.pem()).unwrap();
        write_leaf_hosts(&store, &hosts).unwrap();

        ensure_for_hosts(&settings, &[]).unwrap();

        assert!(certificate_is_current(&store.leaf_path()).unwrap());
        assert_ne!(
            fs::read_to_string(store.leaf_path()).unwrap(),
            expired.pem()
        );
    }

    #[cfg(unix)]
    #[test]
    fn ensure_regenerates_mismatched_ca_key_pair() {
        let temp = tempdir().unwrap();
        let settings = ProxySettings {
            state_dir: Some(temp.path().to_path_buf()),
            ..ProxySettings::default()
        };
        generate(&settings, false).unwrap();
        let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
        let first_ca = fs::read_to_string(store.ca_path()).unwrap();
        let other_key = KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256).unwrap();
        fs::write(store.ca_key_path(), other_key.serialize_pem()).unwrap();

        ensure_for_hosts(&settings, &[]).unwrap();

        assert_ne!(fs::read_to_string(store.ca_path()).unwrap(), first_ca);
        assert!(private_key_matches_certificate(&store.ca_key_path(), &store.ca_path()).unwrap());
    }

    #[cfg(unix)]
    #[test]
    fn ensure_regenerates_mismatched_leaf_key_pair() {
        let temp = tempdir().unwrap();
        let settings = ProxySettings {
            state_dir: Some(temp.path().to_path_buf()),
            ..ProxySettings::default()
        };
        generate(&settings, false).unwrap();
        let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
        let first_cert = fs::read_to_string(store.leaf_path()).unwrap();
        let other_key = KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256).unwrap();
        fs::write(store.leaf_key_path(), other_key.serialize_pem()).unwrap();

        ensure_for_hosts(&settings, &[]).unwrap();

        assert_ne!(fs::read_to_string(store.leaf_path()).unwrap(), first_cert);
        assert!(
            private_key_matches_certificate(&store.leaf_key_path(), &store.leaf_path()).unwrap()
        );
    }

    #[cfg(unix)]
    #[test]
    fn generate_repairs_mismatched_leaf_key_pair_without_force() {
        let temp = tempdir().unwrap();
        let settings = ProxySettings {
            state_dir: Some(temp.path().to_path_buf()),
            ..ProxySettings::default()
        };
        generate(&settings, false).unwrap();
        let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
        let first_cert = fs::read_to_string(store.leaf_path()).unwrap();
        let other_key = KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256).unwrap();
        fs::write(store.leaf_key_path(), other_key.serialize_pem()).unwrap();

        generate(&settings, false).unwrap();

        assert_ne!(fs::read_to_string(store.leaf_path()).unwrap(), first_cert);
        assert!(
            private_key_matches_certificate(&store.leaf_key_path(), &store.leaf_path()).unwrap()
        );
    }

    #[cfg(unix)]
    #[test]
    fn force_generate_refuses_jig_trusted_ca_marker() {
        let temp = tempdir().unwrap();
        let settings = ProxySettings {
            state_dir: Some(temp.path().to_path_buf()),
            ..ProxySettings::default()
        };
        generate(&settings, false).unwrap();
        let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
        write_trusted_ca_marker(&store).unwrap();

        let error = generate(&settings, true).unwrap_err().to_string();

        assert!(error.contains("was trusted by Jig"));
    }

    #[cfg(unix)]
    #[test]
    fn generate_refuses_partial_trusted_ca_pair() {
        let temp = tempdir().unwrap();
        let settings = ProxySettings {
            state_dir: Some(temp.path().to_path_buf()),
            ..ProxySettings::default()
        };
        generate(&settings, false).unwrap();
        let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
        write_trusted_ca_marker(&store).unwrap();
        fs::remove_file(store.ca_key_path()).unwrap();

        let error = generate(&settings, false).unwrap_err().to_string();

        assert!(error.contains("trusted"));
        assert!(!store.ca_key_path().exists());
    }

    #[cfg(unix)]
    #[test]
    fn ensure_refuses_partial_trusted_ca_pair() {
        let temp = tempdir().unwrap();
        let settings = ProxySettings {
            state_dir: Some(temp.path().to_path_buf()),
            ..ProxySettings::default()
        };
        generate(&settings, false).unwrap();
        let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
        write_trusted_ca_marker(&store).unwrap();
        fs::remove_file(store.ca_key_path()).unwrap();

        let error = ensure_for_hosts(&settings, &[]).unwrap_err().to_string();

        assert!(error.contains("trusted"));
        assert!(!store.ca_key_path().exists());
    }

    #[test]
    fn leaf_validity_stays_within_browser_limit() {
        let mut params = CertificateParams::default();
        set_validity(&mut params, LEAF_VALIDITY_DAYS);

        assert!((params.not_after - params.not_before) <= TimeDuration::days(200));
    }

    #[test]
    fn ca_validity_is_bounded_for_local_development_roots() {
        let mut params = CertificateParams::default();
        set_validity(&mut params, CA_VALIDITY_DAYS);

        assert!((params.not_after - params.not_before) <= TimeDuration::days(730));
    }

    #[cfg(unix)]
    #[test]
    fn generated_ca_serials_are_random_128_bit_values() {
        let temp = tempdir().unwrap();
        let settings = ProxySettings {
            state_dir: Some(temp.path().to_path_buf()),
            ..ProxySettings::default()
        };
        generate(&settings, false).unwrap();
        let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
        let first = ca_serial_bytes(&store).unwrap();

        generate(&settings, true).unwrap();
        let second = ca_serial_bytes(&store).unwrap();

        assert_eq!(first.len(), CA_SERIAL_NUMBER_BYTES);
        assert_eq!(second.len(), CA_SERIAL_NUMBER_BYTES);
        assert!(first.iter().any(|byte| *byte != 0));
        assert!(second.iter().any(|byte| *byte != 0));
        assert_ne!(first, second);
    }

    #[cfg(unix)]
    #[test]
    fn force_generate_rotates_leaf_private_key_with_ca() {
        let temp = tempdir().unwrap();
        let settings = ProxySettings {
            state_dir: Some(temp.path().to_path_buf()),
            ..ProxySettings::default()
        };

        generate(&settings, false).unwrap();
        let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
        let first_leaf_key = fs::read_to_string(store.leaf_key_path()).unwrap();

        generate(&settings, true).unwrap();

        assert_ne!(
            fs::read_to_string(store.leaf_key_path()).unwrap(),
            first_leaf_key
        );
        assert!(
            private_key_matches_certificate(&store.leaf_key_path(), &store.leaf_path()).unwrap()
        );
    }

    fn ca_serial_bytes(store: &StateStore) -> Result<Vec<u8>> {
        let der = first_certificate_der(&store.ca_path())?;
        let (_, cert) = parse_x509_certificate(&der)
            .map_err(|error| anyhow!("Failed to parse test CA certificate DER: {error}"))?;
        Ok(cert.tbs_certificate.raw_serial().to_vec())
    }

    #[cfg(unix)]
    #[test]
    fn generate_removes_stale_certificate_temp_files() {
        let temp = tempdir().unwrap();
        let state_dir = temp.path().join("state");
        fs::create_dir_all(&state_dir).unwrap();
        #[cfg(unix)]
        fs::set_permissions(&state_dir, fs::Permissions::from_mode(0o700)).unwrap();
        let stale = state_dir.join("leaf.pem.123.456.0.tmp");
        let stale_trust_extract = state_dir.join("trusted-anchors.pem.123.456.1.tmp");
        fs::write(&stale, "stale").unwrap();
        fs::write(&stale_trust_extract, "stale").unwrap();
        let settings = ProxySettings {
            state_dir: Some(state_dir),
            ..ProxySettings::default()
        };

        generate(&settings, false).unwrap();

        assert!(!stale.exists());
        assert!(!stale_trust_extract.exists());
    }
}
