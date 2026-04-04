use crate::BrewResult;
use crate::download_queue::{self, SharedDownloadProgress};
use crate::global;
use crate::utils::tty;
use anyhow::{Context, anyhow, bail};
use rayon::prelude::*;
use reqwest::blocking::Client;
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue};
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::env;
use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::thread::available_parallelism;
use std::time::Duration;
use url::Url;

const FETCH_RETRIES: usize = 2;
const CONNECT_TIMEOUT_SECS: u64 = 15;
const REQUEST_TIMEOUT_SECS: u64 = 600;
const FALSY_ENV_VALUES: [&str; 5] = ["false", "no", "off", "nil", "0"];

pub(crate) fn fetch_bottles(bottles: &[BottleFetch], client: &Client) -> BrewResult<()> {
    if bottles.len() > 1 {
        println!(
            "Fetching: {}",
            bottles
                .iter()
                .map(|bottle| bottle.formula_name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }

    let download_threads = download_concurrency(bottles.len());
    let show_progress =
        download_queue::should_render_progress(download_queue::download_progress_enabled());
    let progress = bottles
        .iter()
        .map(|bottle| {
            download_queue::new_download_progress(format!(
                "Bottle {} ({})",
                bottle.display_name, bottle.display_version
            ))
        })
        .collect::<Vec<_>>();
    let renderer = show_progress.then(|| download_queue::ProgressRenderer::start(&progress));

    let result = rayon::ThreadPoolBuilder::new()
        .num_threads(download_threads)
        .build()
        .context("Failed to configure download concurrency")?
        .install(|| {
            bottles
                .par_iter()
                .zip(progress.par_iter())
                .try_for_each(|(bottle, progress)| {
                    fetch_bottle(bottle, client, progress, show_progress)
                })
        });

    if let Some(renderer) = renderer {
        renderer.stop();
    }

    result
}

pub(crate) enum Resolution {
    Bottle(Box<ResolvedBottle>),
    Delegate(String),
}

#[derive(Clone, Debug)]
pub(crate) struct ResolvedBottle {
    pub(crate) formula: FormulaJson,
    pub(crate) bottle: BottleFetch,
}

#[derive(Clone, Debug)]
pub(crate) struct BottleFetch {
    pub(crate) formula_name: String,
    pub(crate) display_name: String,
    pub(crate) display_version: String,
    pub(crate) bottle_url: String,
    pub(crate) bottle_sha256: String,
    pub(crate) bottle_cellar: Option<String>,
    pub(crate) cached_download: PathBuf,
    pub(crate) symlink_path: PathBuf,
}

#[derive(Deserialize, Clone, Debug)]
pub(crate) struct FormulaJson {
    pub(crate) name: String,
    pub(crate) full_name: String,
    pub(crate) tap: String,
    pub(crate) versions: Versions,
    pub(crate) revision: u32,
    #[serde(default)]
    pub(crate) post_install_defined: Option<bool>,
    #[serde(default)]
    pub(crate) dependencies: Vec<Value>,
    #[serde(default)]
    pub(crate) build_dependencies: Vec<Value>,
    #[serde(default)]
    pub(crate) test_dependencies: Vec<Value>,
    #[serde(default)]
    pub(crate) recommended_dependencies: Vec<Value>,
    #[serde(default)]
    pub(crate) optional_dependencies: Vec<Value>,
    #[serde(default)]
    pub(crate) uses_from_macos: Vec<Value>,
    #[serde(default)]
    pub(crate) caveats: Option<String>,
    #[serde(default)]
    pub(crate) keg_only_reason: Option<Value>,
    #[serde(default)]
    pub(crate) service: Option<Value>,
    pub(crate) bottle: Option<BottleMetadata>,
}

#[derive(Deserialize, Clone, Debug)]
pub(crate) struct Versions {
    pub(crate) stable: String,
}

#[derive(Deserialize, Clone, Debug)]
pub(crate) struct BottleMetadata {
    pub(crate) stable: Option<BottleStable>,
}

#[derive(Deserialize, Clone, Debug)]
pub(crate) struct BottleStable {
    pub(crate) rebuild: u32,
    pub(crate) files: HashMap<String, BottleFile>,
}

#[derive(Deserialize, Clone, Debug)]
pub(crate) struct BottleFile {
    pub(crate) url: String,
    pub(crate) sha256: String,
    #[serde(default)]
    pub(crate) cellar: Option<String>,
}

#[derive(Deserialize)]
struct SignedPayload {
    payload: String,
}

pub(crate) fn resolve_bottle(
    name: &str,
    aliases: &HashMap<String, String>,
    api_cache: &Path,
    signed_cache_formulae: &mut Option<HashMap<String, FormulaJson>>,
    bottle_tag: &str,
    client: &Client,
) -> BrewResult<Resolution> {
    let resolved_name = aliases.get(name).map(String::as_str).unwrap_or(name);
    let Some(formula) = load_formula_json(resolved_name, api_cache, signed_cache_formulae, client)?
    else {
        return Ok(Resolution::Delegate(format!(
            "formula metadata for `{name}` is not available in the Homebrew API cache."
        )));
    };

    if !is_homebrew_core_tap(&formula.tap) {
        return Ok(Resolution::Delegate(format!(
            "`{}` is not a homebrew/core formula.",
            formula.full_name
        )));
    }

    let Some(bottle) = formula
        .bottle
        .as_ref()
        .and_then(|bottle| bottle.stable.as_ref())
    else {
        return Ok(Resolution::Delegate(format!(
            "`{}` does not have a bottle.",
            formula.full_name
        )));
    };

    let Some((selected_tag, bottle_file)) = bottle
        .files
        .get_key_value(bottle_tag)
        .or_else(|| bottle.files.get_key_value("all"))
    else {
        return Ok(Resolution::Delegate(format!(
            "`{}` does not have a bottle for `{bottle_tag}`.",
            formula.full_name
        )));
    };

    let cache_path = global::cache_path()?;
    let bottle_name = bottle_basename(
        &formula.name,
        &formula.versions.stable,
        formula.revision,
        selected_tag,
        bottle.rebuild,
        &bottle_file.url,
    );

    // Match the path split used by `Bottle#root_url` and `AbstractFileDownloadStrategy`
    // in `Library/Homebrew/bottle.rb`, `Library/Homebrew/utils/bottles.rb`,
    // and `Library/Homebrew/download_strategy.rb`.
    Ok(Resolution::Bottle(Box::new(ResolvedBottle {
        formula: formula.clone(),
        bottle: BottleFetch {
            formula_name: formula.full_name.clone(),
            display_name: formula.name.clone(),
            display_version: pkg_version(&formula.versions.stable, formula.revision),
            bottle_url: bottle_file.url.clone(),
            bottle_sha256: bottle_file.sha256.to_ascii_lowercase(),
            bottle_cellar: bottle_file.cellar.clone(),
            cached_download: cache_path.join("downloads").join(format!(
                "{}--{bottle_name}",
                sha256_hex_str(&bottle_file.url)
            )),
            symlink_path: cache_path.join(format!(
                "{}--{}",
                formula.name,
                pkg_version(&formula.versions.stable, formula.revision)
            )),
        },
    })))
}

pub(crate) fn load_formula_json(
    name: &str,
    api_cache: &Path,
    signed_cache_formulae: &mut Option<HashMap<String, FormulaJson>>,
    client: &Client,
) -> BrewResult<Option<FormulaJson>> {
    let path = api_cache.join("formula").join(format!("{name}.json"));
    if path.exists() {
        return Ok(Some(read_formula_json(&path)?));
    }

    let aggregate_path = api_cache.join("formula.jws.json");
    if aggregate_path.exists() {
        if signed_cache_formulae.is_none() {
            *signed_cache_formulae = Some(load_signed_cache_formulae(&aggregate_path)?);
        }

        return Ok(signed_cache_formulae
            .as_ref()
            .and_then(|formulae| formulae.get(name).cloned()));
    }
    if env_flag("HOMEBREW_USE_INTERNAL_API") {
        return Ok(None);
    }

    let Some(body) = download_formula_json(name, client)? else {
        return Ok(None);
    };

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
    }
    fs::write(&path, body).with_context(|| format!("Failed to write {}", path.display()))?;
    Ok(Some(read_formula_json(&path)?))
}

fn read_formula_json(path: &Path) -> BrewResult<FormulaJson> {
    serde_json::from_str(
        &fs::read_to_string(path).with_context(|| format!("Failed to read {}", path.display()))?,
    )
    .with_context(|| format!("Failed to parse {}", path.display()))
}

fn load_signed_cache_formulae(path: &Path) -> BrewResult<HashMap<String, FormulaJson>> {
    parse_formulae_from_signed_cache(
        &fs::read_to_string(path).with_context(|| format!("Failed to read {}", path.display()))?,
    )
    .with_context(|| format!("Failed to parse {}", path.display()))
}

#[cfg(test)]
fn parse_formula_json_from_signed_cache(
    contents: &str,
    name: &str,
) -> BrewResult<Option<FormulaJson>> {
    Ok(parse_formulae_from_signed_cache(contents)?
        .get(name)
        .cloned())
}

fn parse_formulae_from_signed_cache(contents: &str) -> BrewResult<HashMap<String, FormulaJson>> {
    let signed_payload: SignedPayload = serde_json::from_str(contents)?;
    let formulae: Vec<FormulaJson> = serde_json::from_str(&signed_payload.payload)?;
    let mut indexed_formulae = HashMap::with_capacity(formulae.len() * 2);

    for formula in formulae {
        indexed_formulae.insert(formula.name.clone(), formula.clone());
        indexed_formulae.insert(formula.full_name.clone(), formula);
    }

    Ok(indexed_formulae)
}

fn download_formula_json(name: &str, client: &Client) -> BrewResult<Option<Vec<u8>>> {
    let mut last_error = None;

    for domain in api_domains() {
        let response = match client.get(format!("{domain}/formula/{name}.json")).send() {
            Ok(response) => response,
            Err(error) => {
                last_error = Some(
                    anyhow!(error)
                        .context(format!("Failed to download formula metadata for `{name}`")),
                );
                continue;
            }
        };
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            continue;
        }
        match response.error_for_status() {
            Ok(response) => {
                return Ok(Some(
                    response
                        .bytes()
                        .with_context(|| format!("Failed to read formula metadata for `{name}`"))?
                        .to_vec(),
                ));
            }
            Err(error) => {
                last_error = Some(
                    anyhow!(error)
                        .context(format!("Failed to download formula metadata for `{name}`")),
                );
            }
        }
    }

    if let Some(error) = last_error {
        return Err(error);
    }

    Ok(None)
}

fn fetch_bottle(
    bottle: &BottleFetch,
    client: &Client,
    progress: &SharedDownloadProgress,
    show_progress: bool,
) -> BrewResult<()> {
    if bottle.cached_download.exists() {
        download_queue::update_download_phase(progress, "verifying");
        verify_checksum(&bottle.cached_download, &bottle.bottle_sha256)?;
        ensure_symlink(bottle)?;
        download_queue::mark_download_complete(progress);
        if !show_progress {
            print_downloaded_bottle(bottle);
        }
        return Ok(());
    }

    let temporary_path = temporary_download_path(&bottle.cached_download);
    let mut last_error = None;

    for _ in 0..FETCH_RETRIES {
        let result = (|| {
            if let Some(parent) = bottle.cached_download.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("Failed to create {}", parent.display()))?;
            }
            if temporary_path.exists() {
                fs::remove_file(&temporary_path)
                    .with_context(|| format!("Failed to remove {}", temporary_path.display()))?;
            }

            download_queue::update_download_phase(progress, "downloading");
            match Url::parse(&bottle.bottle_url).context("Failed to parse bottle URL")? {
                url if url.scheme() == "file" => {
                    copy_file_url(&url, &temporary_path, progress)?;
                }
                url => {
                    download_http_url(client, &url, &temporary_path, progress)?;
                }
            }

            download_queue::update_download_phase(progress, "verifying");
            verify_checksum(&temporary_path, &bottle.bottle_sha256)?;
            fs::rename(&temporary_path, &bottle.cached_download).with_context(|| {
                format!(
                    "Failed to move {} to {}",
                    temporary_path.display(),
                    bottle.cached_download.display()
                )
            })?;
            ensure_symlink(bottle)?;
            download_queue::mark_download_complete(progress);
            if !show_progress {
                print_downloaded_bottle(bottle);
            }
            Ok(())
        })();

        match result {
            Ok(()) => return Ok(()),
            Err(error) => {
                last_error = Some(error);
                let _ = fs::remove_file(&temporary_path);
                let _ = fs::remove_file(&bottle.cached_download);
                let _ = fs::remove_file(&bottle.symlink_path);
            }
        }
    }

    Err(last_error.unwrap_or_else(|| anyhow!("Failed to fetch {}", bottle.formula_name)))
}

fn print_downloaded_bottle(bottle: &BottleFetch) {
    let status = format!(
        "{green}✔︎{reset}",
        green = tty::green(),
        reset = tty::reset(),
    );
    println!(
        "{status} Bottle {} ({})",
        bottle.display_name, bottle.display_version
    );
}

fn copy_file_url(
    url: &Url,
    destination: &Path,
    progress: &SharedDownloadProgress,
) -> BrewResult<()> {
    let source = url
        .to_file_path()
        .map_err(|_| anyhow!("Failed to convert {url} to a file path"))?;
    let mut input =
        File::open(&source).with_context(|| format!("Failed to open {}", source.display()))?;
    download_queue::update_download_total(
        progress,
        input.metadata().ok().map(|metadata| metadata.len()),
    );
    let mut output = File::create(destination)
        .with_context(|| format!("Failed to create {}", destination.display()))?;
    copy_stream(&mut input, &mut output, progress)?;
    Ok(())
}

fn download_http_url(
    client: &Client,
    url: &Url,
    destination: &Path,
    progress: &SharedDownloadProgress,
) -> BrewResult<()> {
    let auth_header = resolve_http_auth(url);
    let mut request = client.get(url.as_str());
    if let Some(auth) = &auth_header {
        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(auth).context("Failed to build authorization header")?,
        );
        request = request.headers(headers);
    }

    let mut response = request
        .send()
        .with_context(|| format!("Failed to download {url}"))?
        .error_for_status()
        .with_context(|| format!("Failed to download {url}"))?;
    download_queue::update_download_total(progress, response.content_length());
    let mut output = File::create(destination)
        .with_context(|| format!("Failed to create {}", destination.display()))?;
    copy_stream(&mut response, &mut output, progress)?;
    Ok(())
}

const GHCR_ANONYMOUS_TOKEN: &str = "Bearer QQ==";

fn resolve_http_auth(url: &Url) -> Option<String> {
    if !should_send_github_packages_auth(url) {
        return None;
    }
    Some(
        env_value("HOMEBREW_GITHUB_PACKAGES_AUTH")
            .unwrap_or_else(|| GHCR_ANONYMOUS_TOKEN.to_string()),
    )
}

fn copy_stream(
    input: &mut dyn Read,
    output: &mut dyn Write,
    progress: &SharedDownloadProgress,
) -> BrewResult<()> {
    let mut buffer = [0_u8; 16 * 1024];
    loop {
        let read = input
            .read(&mut buffer)
            .context("Failed to read download data")?;
        if read == 0 {
            break;
        }
        output
            .write_all(&buffer[..read])
            .context("Failed to write download data")?;
        download_queue::increment_downloaded_size(progress, read as u64);
    }
    output.flush().context("Failed to flush download data")?;
    Ok(())
}

fn ensure_symlink(bottle: &BottleFetch) -> BrewResult<()> {
    if let Some(parent) = bottle.symlink_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
    }
    if bottle.symlink_path.exists() || bottle.symlink_path.is_symlink() {
        fs::remove_file(&bottle.symlink_path)
            .with_context(|| format!("Failed to remove {}", bottle.symlink_path.display()))?;
    }
    symlink(
        Path::new("downloads").join(
            bottle
                .cached_download
                .file_name()
                .ok_or_else(|| anyhow!("Missing cached bottle name for {}", bottle.formula_name))?,
        ),
        &bottle.symlink_path,
    )
    .with_context(|| format!("Failed to create {}", bottle.symlink_path.display()))?;
    Ok(())
}

fn verify_checksum(path: &Path, expected: &str) -> BrewResult<()> {
    let mut file =
        File::open(path).with_context(|| format!("Failed to open {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 16 * 1024];

    loop {
        let read = file
            .read(&mut buffer)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }

    let actual = lower_hex(hasher.finalize());
    if actual == expected {
        return Ok(());
    }

    bail!(
        "SHA-256 mismatch for {}: expected {}, got {}",
        path.display(),
        expected,
        actual
    )
}

pub(crate) fn load_aliases(path: &Path) -> BrewResult<HashMap<String, String>> {
    if !path.exists() {
        return Ok(HashMap::new());
    }

    Ok(global::read_lines(path)?
        .into_iter()
        .filter_map(|line| {
            line.split_once('|')
                .map(|(alias, canonical)| (alias.to_string(), canonical.to_string()))
        })
        .collect())
}

pub(crate) fn build_client() -> BrewResult<Client> {
    let pkg_name_and_version = format!("{}/{}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
    let user_agent = match env_value("HOMEBREW_USER_AGENT") {
        Some(brew_ua) => format!("{brew_ua} {pkg_name_and_version}"),
        None => pkg_name_and_version,
    };

    Client::builder()
        .connect_timeout(Duration::from_secs(CONNECT_TIMEOUT_SECS))
        .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
        .user_agent(user_agent)
        .gzip(true)
        .deflate(true)
        .build()
        .context("Failed to build an HTTP client")
}

fn api_domains() -> Vec<String> {
    let default_domain = env::var("HOMEBREW_API_DEFAULT_DOMAIN")
        .unwrap_or_else(|_| "https://formulae.brew.sh/api".to_string());
    match env::var("HOMEBREW_API_DOMAIN") {
        Ok(domain) if domain != default_domain => vec![domain, default_domain],
        Ok(domain) => vec![domain],
        Err(_) => vec![default_domain],
    }
}

pub(crate) fn current_bottle_tag() -> BrewResult<String> {
    if env_flag("HOMEBREW_LINUX")
        || env::var("HOMEBREW_SYSTEM")
            .map(|system| system == "Linux")
            .unwrap_or(false)
    {
        return Ok(format!(
            "{}_linux",
            env::var("HOMEBREW_PHYSICAL_PROCESSOR")
                .or_else(|_| env::var("HOMEBREW_PROCESSOR"))
                .context("HOMEBREW_PHYSICAL_PROCESSOR is not set")?
        ));
    }

    let processor = env::var("HOMEBREW_PHYSICAL_PROCESSOR")
        .or_else(|_| env::var("HOMEBREW_PROCESSOR"))
        .context("HOMEBREW_PHYSICAL_PROCESSOR is not set")?;
    let version = env::var("HOMEBREW_MACOS_VERSION_NUMERIC")
        .context("HOMEBREW_MACOS_VERSION_NUMERIC is not set")?
        .parse::<u32>()
        .context("HOMEBREW_MACOS_VERSION_NUMERIC is invalid")?;
    let macos = macos_version_name(version).ok_or_else(|| anyhow!("Unsupported macOS version"))?;

    if processor == "x86_64" {
        Ok(macos.to_string())
    } else {
        Ok(format!("{processor}_{macos}"))
    }
}

fn macos_version_name(version: u32) -> Option<&'static str> {
    if version >= 260000 {
        Some("tahoe")
    } else if version >= 150000 {
        Some("sequoia")
    } else if version >= 140000 {
        Some("sonoma")
    } else if version >= 130000 {
        Some("ventura")
    } else if version >= 120000 {
        Some("monterey")
    } else if version >= 110000 {
        Some("big_sur")
    } else if version >= 101500 {
        Some("catalina")
    } else {
        None
    }
}

fn bottle_basename(
    name: &str,
    version: &str,
    revision: u32,
    tag: &str,
    rebuild: u32,
    url: &str,
) -> String {
    let pkg_version = pkg_version(version, revision);
    let ext = if rebuild == 0 {
        format!(".{tag}.bottle.tar.gz")
    } else {
        format!(".{tag}.bottle.{rebuild}.tar.gz")
    };
    let filename = format!("{name}--{pkg_version}{ext}");

    if url.contains("/blobs/sha256:") {
        filename
    } else {
        url::form_urlencoded::byte_serialize(filename.as_bytes()).collect()
    }
}

pub(crate) fn pkg_version(version: &str, revision: u32) -> String {
    if revision == 0 {
        version.to_string()
    } else {
        format!("{version}_{revision}")
    }
}

fn download_concurrency(requested_downloads: usize) -> usize {
    parse_download_concurrency(
        env::var("HOMEBREW_DOWNLOAD_CONCURRENCY").ok().as_deref(),
        available_parallelism().map(usize::from).unwrap_or(1),
        requested_downloads,
    )
}

pub(crate) fn is_simple_formula_name(name: &str) -> bool {
    !name.is_empty()
        && !name.contains('/')
        && !name.contains(':')
        && !name.contains("://")
        && name.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '@' | '+' | '.' | '_' | '-')
        })
}

fn is_homebrew_core_tap(tap: &str) -> bool {
    tap.eq_ignore_ascii_case("homebrew/core") || tap.eq_ignore_ascii_case("homebrew/homebrew-core")
}

fn env_value(name: &str) -> Option<String> {
    env::var(name).ok().filter(|value| !value.is_empty())
}

fn env_flag(name: &str) -> bool {
    is_truthy_env_value(env_value(name).as_deref())
}

fn is_truthy_env_value(value: Option<&str>) -> bool {
    value
        .map(|value| {
            FALSY_ENV_VALUES
                .iter()
                .all(|falsy| !value.eq_ignore_ascii_case(falsy))
        })
        .unwrap_or(false)
}

fn parse_download_concurrency(
    value: Option<&str>,
    available_threads: usize,
    requested_downloads: usize,
) -> usize {
    let concurrency = match value {
        Some("auto") | None => available_threads.saturating_mul(2),
        Some(value) => value
            .parse::<usize>()
            .ok()
            .filter(|value| *value > 0)
            .unwrap_or(1),
    };

    concurrency.max(1).min(requested_downloads.max(1))
}

fn should_send_github_packages_auth(url: &Url) -> bool {
    should_send_github_packages_auth_for_host(
        url.host_str(),
        env_value("HOMEBREW_GITHUB_PACKAGES_AUTH").is_some(),
        env_value("HOMEBREW_ARTIFACT_DOMAIN").is_some(),
        env_value("HOMEBREW_DOCKER_REGISTRY_BASIC_AUTH_TOKEN").is_some(),
        env_value("HOMEBREW_DOCKER_REGISTRY_TOKEN").is_some(),
    )
}

fn should_send_github_packages_auth_for_host(
    host: Option<&str>,
    github_packages_auth_present: bool,
    artifact_domain_present: bool,
    docker_registry_basic_auth_token_present: bool,
    docker_registry_token_present: bool,
) -> bool {
    host.is_some_and(|host| host.eq_ignore_ascii_case("ghcr.io"))
        && github_packages_auth_present
        && (!artifact_domain_present
            || docker_registry_basic_auth_token_present
            || docker_registry_token_present)
}

fn temporary_download_path(path: &Path) -> PathBuf {
    let mut incomplete = OsString::from(path.as_os_str());
    incomplete.push(".incomplete");
    PathBuf::from(incomplete)
}

fn lower_hex(value: impl AsRef<[u8]>) -> String {
    value
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn sha256_hex_str(value: &str) -> String {
    lower_hex(Sha256::digest(value.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::{
        bottle_basename, is_homebrew_core_tap, is_simple_formula_name, is_truthy_env_value,
        parse_download_concurrency, parse_formula_json_from_signed_cache, pkg_version,
        sha256_hex_str, should_send_github_packages_auth_for_host, temporary_download_path,
    };
    use std::ffi::OsString;
    use std::os::unix::ffi::{OsStrExt, OsStringExt};
    use std::path::PathBuf;

    #[test]
    fn allows_simple_formula_names() {
        assert!(is_simple_formula_name("python@3.12"));
        assert!(is_simple_formula_name("llvm+mlir"));
        assert!(!is_simple_formula_name("homebrew/core/jq"));
        assert!(!is_simple_formula_name("https://example.com/foo.rb"));
    }

    #[test]
    fn appends_formula_revisions_to_pkg_versions() {
        assert_eq!(pkg_version("1.2.3", 0), "1.2.3");
        assert_eq!(pkg_version("1.2.3", 1), "1.2.3_1");
    }

    #[test]
    fn uses_github_packages_bottle_names_for_ghcr_urls() {
        assert_eq!(
            bottle_basename(
                "jq",
                "1.8.1",
                0,
                "x86_64_linux",
                0,
                "https://ghcr.io/v2/homebrew/core/jq/blobs/sha256:deadbeef",
            ),
            "jq--1.8.1.x86_64_linux.bottle.tar.gz"
        );
    }

    #[test]
    fn url_encodes_non_ghcr_bottle_names() {
        assert_eq!(
            bottle_basename(
                "openssl@3",
                "3.6.1",
                0,
                "x86_64_linux",
                0,
                "https://example.com/bottles/openssl%403--3.6.1.x86_64_linux.bottle.tar.gz",
            ),
            "openssl%403--3.6.1.x86_64_linux.bottle.tar.gz"
        );
    }

    #[test]
    fn caps_download_concurrency_at_the_number_of_requested_downloads() {
        assert_eq!(parse_download_concurrency(Some("auto"), 4, 3), 3);
        assert_eq!(parse_download_concurrency(None, 4, 10), 8);
        assert_eq!(parse_download_concurrency(Some("2"), 4, 10), 2);
    }

    #[test]
    fn accepts_both_homebrew_core_tap_spellings() {
        assert!(is_homebrew_core_tap("homebrew/core"));
        assert!(is_homebrew_core_tap("Homebrew/homebrew-core"));
        assert!(is_homebrew_core_tap("HoMeBrEw/CoRe"));
        assert!(is_homebrew_core_tap("hOmEbReW/HoMeBrEw-CoRe"));
        assert!(!is_homebrew_core_tap("someone/else"));
    }

    #[test]
    fn reads_formulae_from_the_signed_api_cache() {
        let formula = parse_formula_json_from_signed_cache(
            r#"{"payload":"[{\"name\":\"wget\",\"full_name\":\"wget\",\"tap\":\"Homebrew/homebrew-core\",\"versions\":{\"stable\":\"1.25.0\"},\"revision\":0,\"bottle\":{\"stable\":{\"rebuild\":0,\"files\":{}}}}]"}"#,
            "wget",
        )
        .unwrap()
        .unwrap();

        assert_eq!(formula.tap, "Homebrew/homebrew-core");
    }

    #[test]
    fn treats_falsey_homebrew_boolean_env_values_as_false() {
        assert!(!is_truthy_env_value(None));
        assert!(!is_truthy_env_value(Some("0")));
        assert!(!is_truthy_env_value(Some("false")));
        assert!(!is_truthy_env_value(Some("No")));
        assert!(is_truthy_env_value(Some("1")));
        assert!(is_truthy_env_value(Some("true")));
    }

    #[test]
    fn limits_github_packages_auth_to_ghcr_when_ruby_would_send_it() {
        assert!(should_send_github_packages_auth_for_host(
            Some("ghcr.io"),
            true,
            false,
            false,
            false,
        ));
        assert!(!should_send_github_packages_auth_for_host(
            Some("example.com"),
            true,
            false,
            false,
            false,
        ));
        assert!(!should_send_github_packages_auth_for_host(
            Some("ghcr.io"),
            true,
            true,
            false,
            false,
        ));
        assert!(should_send_github_packages_auth_for_host(
            Some("ghcr.io"),
            true,
            true,
            false,
            true,
        ));
    }

    #[test]
    fn formats_sha256_values_as_lowercase_hex() {
        assert_eq!(
            sha256_hex_str("Homebrew"),
            "bc49b968dbe459ee5b7d2299c057a3724d4088c1fc6bb0be8ac8f9aaf2c50020"
        );
    }

    #[test]
    fn preserves_non_utf8_bytes_in_temporary_download_paths() {
        let path = PathBuf::from(OsString::from_vec(vec![0x66, 0x6f, 0x80, 0x6f]));
        let temporary_path = temporary_download_path(&path);

        assert_eq!(
            temporary_path.as_os_str().as_bytes(),
            &[
                0x66, 0x6f, 0x80, 0x6f, b'.', b'i', b'n', b'c', b'o', b'm', b'p', b'l', b'e', b't',
                b'e'
            ]
        );
    }
}
