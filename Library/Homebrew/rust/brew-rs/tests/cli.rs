use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

struct TestContext {
    _tempdir: TestDir,
    brew_file: PathBuf,
    cache: PathBuf,
    cellar: PathBuf,
    caskroom: PathBuf,
    prefix: PathBuf,
}

impl TestContext {
    fn new() -> Self {
        let tempdir = new_tempdir();
        let temp_root = tempdir.path().to_path_buf();
        let repo_root = repo_root();
        let prefix = temp_root.join("prefix");
        let brew_file = prefix.join("bin/brew");

        fs::create_dir_all(brew_file.parent().unwrap()).unwrap();
        fs::copy(repo_root.join("bin/brew"), &brew_file).unwrap();
        symlink(repo_root.join("Library"), prefix.join("Library")).unwrap();

        Self {
            _tempdir: tempdir,
            brew_file,
            cache: temp_root.join("cache"),
            cellar: prefix.join("Cellar"),
            caskroom: prefix.join("Caskroom"),
            prefix,
        }
    }

    fn formula_path(&self) -> PathBuf {
        let path = self.prefix.join("rust-parity-testball.rb");
        fs::write(
            &path,
            r#"
class RustParityTestball < Formula
  desc "Rust parity fixture"
  homepage "https://brew.sh"
  url "https://example.com/rust-parity-testball-1.0.tar.gz"
  sha256 "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
end
"#
            .trim_start(),
        )
        .unwrap();
        path
    }

    fn formula_api_path(&self, name: &str) -> PathBuf {
        self.cache.join("api/formula").join(format!("{name}.json"))
    }

    fn ruby_command(&self) -> Command {
        let mut command = Command::new(&self.brew_file);
        command.current_dir(repo_root());
        for (key, value) in self.common_env() {
            command.env(key, value);
        }
        command
    }

    fn gated_rust_command(&self) -> Command {
        let mut command = Command::new(&self.brew_file);
        command.current_dir(repo_root());
        for (key, value) in self.common_env() {
            command.env(key, value);
        }
        command.env("HOMEBREW_EXPERIMENTAL_RUST_FRONTEND", "1");
        command
    }

    fn rust_command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_brew-rs"));
        command.current_dir(repo_root());
        for (key, value) in self.common_env() {
            command.env(key, value);
        }
        command
    }

    fn common_env(&self) -> [(OsString, OsString); 13] {
        [
            (
                "HOMEBREW_BREW_FILE".into(),
                self.brew_file.clone().into_os_string(),
            ),
            ("HOMEBREW_CACHE".into(), self.cache.clone().into_os_string()),
            (
                "HOMEBREW_CASKROOM".into(),
                self.caskroom.clone().into_os_string(),
            ),
            (
                "HOMEBREW_CELLAR".into(),
                self.cellar.clone().into_os_string(),
            ),
            ("HOMEBREW_DEVELOPER".into(), OsString::from("1")),
            ("HOMEBREW_LINUX".into(), OsString::from("1")),
            (
                "HOMEBREW_MACOS_VERSION_NUMERIC".into(),
                OsString::from("000000"),
            ),
            ("HOMEBREW_NO_AUTO_UPDATE".into(), OsString::from("1")),
            ("HOMEBREW_NO_COLOR".into(), OsString::from("1")),
            ("HOMEBREW_NO_ENV_HINTS".into(), OsString::from("1")),
            (
                "HOMEBREW_PHYSICAL_PROCESSOR".into(),
                OsString::from("x86_64"),
            ),
            (
                "HOMEBREW_PREFIX".into(),
                self.prefix.clone().into_os_string(),
            ),
            ("HOMEBREW_SYSTEM".into(), OsString::from("Linux")),
        ]
    }
}

#[test]
fn search_uses_the_rust_search_flow() {
    let context = TestContext::new();
    let api_cache = context.cache.join("api");

    fs::create_dir_all(&api_cache).unwrap();
    fs::write(api_cache.join("formula_names.txt"), "testball\n").unwrap();
    fs::write(api_cache.join("cask_names.txt"), "local-caffeine\n").unwrap();

    let output = context
        .rust_command()
        .args(["search", "l"])
        .output()
        .unwrap();

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "testball\n\nlocal-caffeine\n"
    );
}

#[test]
fn brew_sh_routes_supported_commands_to_brew_rs_outside_the_default_prefix() {
    let context = TestContext::new();
    let api_cache = context.cache.join("api");

    fs::create_dir_all(&api_cache).unwrap();
    fs::write(api_cache.join("formula_names.txt"), "testball\n").unwrap();
    fs::write(api_cache.join("cask_names.txt"), "").unwrap();

    let output = context
        .gated_rust_command()
        .args(["search", "testbal"])
        .output()
        .unwrap();

    assert!(output.status.success(), "{output:?}");
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "testball\n");
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("using the experimental brew-rs Rust frontend."),
    );
}

#[test]
fn search_uses_fuzzy_fallback_when_plain_text_search_has_no_exact_matches() {
    let context = TestContext::new();
    let api_cache = context.cache.join("api");

    fs::create_dir_all(&api_cache).unwrap();
    fs::write(api_cache.join("formula_names.txt"), "testball\n").unwrap();
    fs::write(api_cache.join("cask_names.txt"), "").unwrap();

    let output = context
        .rust_command()
        .args(["search", "testbal"])
        .output()
        .unwrap();

    assert!(output.status.success());
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "testball\n");
}

#[test]
fn info_matches_the_ruby_backend_output() {
    let context = TestContext::new();
    let formula = context.formula_path();

    let ruby_output = context
        .ruby_command()
        .args(["info", formula.to_str().unwrap()])
        .output()
        .unwrap();
    let rust_output = context
        .rust_command()
        .args(["info", formula.to_str().unwrap()])
        .output()
        .unwrap();

    assert!(ruby_output.status.success());
    assert!(rust_output.status.success());
    assert_eq!(rust_output.stdout, ruby_output.stdout);
    assert!(
        String::from_utf8(rust_output.stderr)
            .unwrap()
            .contains("Warning: brew-rs is handing info back to the Ruby backend.")
    );
}

#[test]
fn list_matches_the_ruby_backend_output_for_named_formula() {
    let context = TestContext::new();

    fs::create_dir_all(context.cellar.join("foo/1.0/bin")).unwrap();
    fs::write(context.cellar.join("foo/1.0/bin/foo"), "foo").unwrap();

    let ruby_output = context
        .ruby_command()
        .args(["list", "foo"])
        .output()
        .unwrap();
    let rust_output = context
        .rust_command()
        .args(["list", "foo"])
        .output()
        .unwrap();

    assert!(ruby_output.status.success());
    assert!(rust_output.status.success());
    assert_eq!(rust_output.stdout, ruby_output.stdout);
}

#[test]
fn list_uses_the_linked_keg_when_listing_formula_files() {
    let context = TestContext::new();
    let linked_keg = context.cellar.join("linked-formula/1.0");

    fs::create_dir_all(linked_keg.join("bin")).unwrap();
    fs::write(linked_keg.join("bin/linked-formula"), "foo").unwrap();
    fs::create_dir_all(context.cellar.join("linked-formula/2.0/bin")).unwrap();
    fs::write(
        context
            .cellar
            .join("linked-formula/2.0/bin/linked-formula-newer"),
        "foo",
    )
    .unwrap();
    fs::create_dir_all(context.prefix.join("opt")).unwrap();
    symlink(&linked_keg, context.prefix.join("opt/linked-formula")).unwrap();

    let ruby_output = context
        .ruby_command()
        .args(["list", "linked-formula"])
        .output()
        .unwrap();
    let rust_output = context
        .rust_command()
        .args(["list", "linked-formula"])
        .output()
        .unwrap();

    assert!(ruby_output.status.success());
    assert!(rust_output.status.success());
    assert_eq!(rust_output.stdout, ruby_output.stdout);
}

#[test]
fn fetch_downloads_a_homebrew_core_bottle_to_the_homebrew_cache() {
    let context = TestContext::new();
    let bottle_source = context
        .prefix
        .join("testball--1.0.x86_64_linux.bottle.tar.gz");
    let bottle_contents = b"testball bottle";
    let bottle_sha256 = sha256_hex(bottle_contents);
    let bottle_url_sha256 = sha256_hex(format!("file://{}", bottle_source.display()).as_bytes());

    fs::write(&bottle_source, bottle_contents).unwrap();
    fs::create_dir_all(context.formula_api_path("testball").parent().unwrap()).unwrap();
    fs::write(
        context.formula_api_path("testball"),
        format!(
            r#"{{
  "name": "testball",
  "full_name": "testball",
  "tap": "homebrew/core",
  "versions": {{
    "stable": "1.0"
  }},
  "revision": 0,
  "bottle": {{
    "stable": {{
      "rebuild": 0,
      "files": {{
        "x86_64_linux": {{
          "url": "file://{}",
          "sha256": "{}"
        }}
      }}
    }}
  }}
}}"#,
            bottle_source.display(),
            bottle_sha256,
        ),
    )
    .unwrap();

    let output = context
        .rust_command()
        .args(["fetch", "testball"])
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout.clone()).unwrap();

    assert!(output.status.success(), "{output:?}");
    assert!(stdout.contains("Bottle testball (1.0)"), "{stdout}");
    assert!(context.cache.join("testball--1.0").is_symlink());
    assert_eq!(
        fs::read_link(context.cache.join("testball--1.0")).unwrap(),
        PathBuf::from(format!(
            "downloads/{bottle_url_sha256}--testball--1.0.x86_64_linux.bottle.tar.gz"
        ))
    );
    assert_eq!(
        fs::read(context.cache.join("downloads").join(format!(
            "{bottle_url_sha256}--testball--1.0.x86_64_linux.bottle.tar.gz"
        )),)
        .unwrap(),
        bottle_contents
    );
}

#[test]
fn fetch_downloads_multiple_bottles_in_one_invocation() {
    let context = TestContext::new();

    for name in ["testball1", "testball2"] {
        let bottle_source = context
            .prefix
            .join(format!("{name}--1.0.x86_64_linux.bottle.tar.gz"));
        let bottle_contents = format!("{name} bottle");
        let bottle_sha256 = sha256_hex(bottle_contents.as_bytes());

        fs::write(&bottle_source, bottle_contents).unwrap();
        fs::create_dir_all(context.formula_api_path(name).parent().unwrap()).unwrap();
        fs::write(
            context.formula_api_path(name),
            format!(
                r#"{{
  "name": "{name}",
  "full_name": "{name}",
  "tap": "homebrew/core",
  "versions": {{
    "stable": "1.0"
  }},
  "revision": 0,
  "bottle": {{
    "stable": {{
      "rebuild": 0,
      "files": {{
        "x86_64_linux": {{
          "url": "file://{}",
          "sha256": "{}"
        }}
      }}
    }}
  }}
}}"#,
                bottle_source.display(),
                bottle_sha256,
            ),
        )
        .unwrap();
    }

    let output = context
        .rust_command()
        .env("HOMEBREW_DOWNLOAD_CONCURRENCY", "2")
        .args(["fetch", "testball1", "testball2"])
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout.clone()).unwrap();

    assert!(output.status.success(), "{output:?}");
    assert!(stdout.contains("Bottle testball1 (1.0)"), "{stdout}");
    assert!(stdout.contains("Bottle testball2 (1.0)"), "{stdout}");
    assert!(context.cache.join("testball1--1.0").is_symlink());
    assert!(context.cache.join("testball2--1.0").is_symlink());
}

#[test]
fn fetch_deduplicates_duplicate_formula_names() {
    let context = TestContext::new();

    let bottle_source = context
        .prefix
        .join("testball--1.0.x86_64_linux.bottle.tar.gz");
    let bottle_contents = b"testball bottle";
    let bottle_sha256 = sha256_hex(bottle_contents);

    fs::write(&bottle_source, bottle_contents).unwrap();
    fs::create_dir_all(context.formula_api_path("testball").parent().unwrap()).unwrap();
    fs::write(
        context.formula_api_path("testball"),
        format!(
            r#"{{
  "name": "testball",
  "full_name": "testball",
  "tap": "homebrew/core",
  "versions": {{
    "stable": "1.0"
  }},
  "revision": 0,
  "bottle": {{
    "stable": {{
      "rebuild": 0,
      "files": {{
        "x86_64_linux": {{
          "url": "file://{}",
          "sha256": "{}"
        }}
      }}
    }}
  }}
}}"#,
            bottle_source.display(),
            bottle_sha256,
        ),
    )
    .unwrap();

    let output = context
        .rust_command()
        .args(["fetch", "testball", "testball"])
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout.clone()).unwrap();

    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        stdout.matches("Bottle testball (1.0)").count(),
        1,
        "{stdout}"
    );
}

#[test]
fn fetch_delegates_to_ruby_with_a_reason_when_flags_are_used() {
    let context = TestContext::new();

    let output = context
        .rust_command()
        .args(["fetch", "--help"])
        .output()
        .unwrap();

    assert!(output.status.success(), "{output:?}");
    assert!(String::from_utf8(output.stderr).unwrap().contains(
        "Warning: brew-rs is handing fetch back to the Ruby backend: flags are not yet supported."
    ),);
}

#[test]
fn fetch_delegates_to_ruby_with_a_reason_when_a_bottle_is_unavailable() {
    let context = TestContext::new();

    fs::create_dir_all(context.formula_api_path("testball").parent().unwrap()).unwrap();
    fs::write(
        context.formula_api_path("testball"),
        r#"{
  "name": "testball",
  "full_name": "testball",
  "tap": "homebrew/core",
  "versions": {
    "stable": "1.0"
  },
  "revision": 0,
  "bottle": {
    "stable": {
      "rebuild": 0,
      "files": {}
    }
  }
}"#,
    )
    .unwrap();

    let output = context
        .rust_command()
        .args(["fetch", "testball"])
        .output()
        .unwrap();

    assert!(!output.status.success(), "{output:?}");
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("Warning: brew-rs is handing fetch back to the Ruby backend: `testball` does not have a bottle for `x86_64_linux`."),
    );
}

#[test]
fn install_pours_and_links_a_basic_bottle_without_persisting_tab_or_sbom() {
    let context = TestContext::new();
    let bottle_staging_root = context.cache.join("bottle-staging");
    let bottle_root = bottle_staging_root.join("testball/1.0");
    let bottle_source = context
        .cache
        .join("testball--1.0.x86_64_linux.bottle.tar.gz");

    fs::create_dir_all(bottle_root.join("bin")).unwrap();
    fs::create_dir_all(bottle_root.join(".brew")).unwrap();
    fs::write(bottle_root.join("INSTALL_RECEIPT.json"), "{}").unwrap();
    fs::write(bottle_root.join("sbom.spdx.json"), "{}").unwrap();
    fs::write(
        bottle_root.join(".brew/testball.rb"),
        "class Testball < Formula\nend\n",
    )
    .unwrap();
    fs::write(bottle_root.join("bin/testball"), "#!/bin/sh\nexit 0\n").unwrap();
    fs::set_permissions(
        bottle_root.join("bin/testball"),
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();

    let status = Command::new("tar")
        .args([
            "-czf",
            bottle_source.to_str().unwrap(),
            "-C",
            bottle_staging_root.to_str().unwrap(),
            "testball",
        ])
        .status()
        .unwrap();
    assert!(status.success());

    let bottle_sha256 = sha256_hex(fs::read(&bottle_source).unwrap());

    fs::create_dir_all(context.formula_api_path("testball").parent().unwrap()).unwrap();
    fs::write(
        context.formula_api_path("testball"),
        format!(
            r#"{{
  "name": "testball",
  "full_name": "testball",
  "tap": "homebrew/core",
  "versions": {{
    "stable": "1.0"
  }},
  "revision": 0,
  "post_install_defined": false,
  "dependencies": [],
  "build_dependencies": [],
  "recommended_dependencies": [],
  "optional_dependencies": [],
  "uses_from_macos": [],
  "bottle": {{
    "stable": {{
      "rebuild": 0,
      "files": {{
        "x86_64_linux": {{
          "url": "file://{}",
          "sha256": "{}",
          "cellar": ":any_skip_relocation"
        }}
      }}
    }}
  }}
}}"#,
            bottle_source.display(),
            bottle_sha256,
        ),
    )
    .unwrap();

    let output = context
        .rust_command()
        .args(["install", "testball"])
        .output()
        .unwrap();

    assert!(output.status.success(), "{output:?}");
    assert!(
        !String::from_utf8(output.stderr)
            .unwrap()
            .contains("Warning: brew-rs is handing install back to the Ruby backend."),
    );
    assert!(context.cellar.join("testball/1.0/bin/testball").exists());
    assert!(context.prefix.join("bin/testball").exists());
    assert!(context.prefix.join("opt/testball").is_symlink());
    assert!(
        !context
            .cellar
            .join("testball/1.0/INSTALL_RECEIPT.json")
            .exists()
    );
    assert!(!context.cellar.join("testball/1.0/sbom.spdx.json").exists());
}

#[test]
fn install_pours_and_links_a_bottle_formula_with_a_simple_runtime_dependency() {
    let context = TestContext::new();
    let bottle_staging_root = context.cache.join("bottle-staging");

    for name in ["depball", "testball"] {
        let bottle_root = bottle_staging_root.join(format!("{name}/1.0"));
        let bottle_source = context
            .cache
            .join(format!("{name}--1.0.x86_64_linux.bottle.tar.gz"));

        fs::create_dir_all(bottle_root.join("bin")).unwrap();
        fs::create_dir_all(bottle_root.join(".brew")).unwrap();
        fs::write(
            bottle_root.join(format!("bin/{name}")),
            "#!/bin/sh\nexit 0\n",
        )
        .unwrap();
        fs::write(
            bottle_root.join(format!(".brew/{name}.rb")),
            if name == "depball" {
                "class Depball < Formula\nend\n".to_string()
            } else {
                "class Testball < Formula\nend\n".to_string()
            },
        )
        .unwrap();
        fs::set_permissions(
            bottle_root.join(format!("bin/{name}")),
            std::fs::Permissions::from_mode(0o755),
        )
        .unwrap();

        let status = Command::new("tar")
            .args([
                "-czf",
                bottle_source.to_str().unwrap(),
                "-C",
                bottle_staging_root.to_str().unwrap(),
                name,
            ])
            .status()
            .unwrap();
        assert!(status.success());

        fs::create_dir_all(context.formula_api_path(name).parent().unwrap()).unwrap();
        fs::write(
            context.formula_api_path(name),
            format!(
                r#"{{
  "name": "{name}",
  "full_name": "{name}",
  "tap": "homebrew/core",
  "versions": {{
    "stable": "1.0"
  }},
  "revision": 0,
  "post_install_defined": false,
  "dependencies": [{dependencies}],
  "build_dependencies": [],
  "recommended_dependencies": [],
  "optional_dependencies": [],
  "uses_from_macos": [],
  "bottle": {{
    "stable": {{
      "rebuild": 0,
      "files": {{
        "x86_64_linux": {{
          "url": "file://{}",
          "sha256": "{}",
          "cellar": ":any_skip_relocation"
        }}
      }}
    }}
  }}
}}"#,
                bottle_source.display(),
                sha256_hex(fs::read(&bottle_source).unwrap()),
                dependencies = if name == "testball" {
                    "\"depball\""
                } else {
                    ""
                },
            ),
        )
        .unwrap();
    }

    let output = context
        .rust_command()
        .args(["install", "testball"])
        .output()
        .unwrap();

    assert!(output.status.success(), "{output:?}");
    assert!(
        !String::from_utf8(output.stderr)
            .unwrap()
            .contains("Warning: brew-rs is handing install back to the Ruby backend."),
    );
    assert!(context.cellar.join("depball/1.0/bin/depball").exists());
    assert!(context.prefix.join("bin/depball").exists());
    assert!(context.prefix.join("opt/depball").is_symlink());
    assert!(context.cellar.join("testball/1.0/bin/testball").exists());
    assert!(context.prefix.join("bin/testball").exists());
    assert!(context.prefix.join("opt/testball").is_symlink());
}

#[test]
fn install_cleans_up_when_a_bottle_extracts_an_unexpected_prefix() {
    let context = TestContext::new();
    let bottle_staging_root = context.cache.join("bottle-staging");
    let bottle_root = bottle_staging_root.join("wrongball/1.0");
    let bottle_source = context
        .cache
        .join("testball--1.0.x86_64_linux.bottle.tar.gz");

    fs::create_dir_all(bottle_root.join("bin")).unwrap();
    fs::write(bottle_root.join("bin/testball"), "#!/bin/sh\nexit 0\n").unwrap();
    fs::set_permissions(
        bottle_root.join("bin/testball"),
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();

    let status = Command::new("tar")
        .args([
            "-czf",
            bottle_source.to_str().unwrap(),
            "-C",
            bottle_staging_root.to_str().unwrap(),
            "wrongball",
        ])
        .status()
        .unwrap();
    assert!(status.success());

    let bottle_sha256 = sha256_hex(fs::read(&bottle_source).unwrap());

    fs::create_dir_all(context.formula_api_path("testball").parent().unwrap()).unwrap();
    fs::write(
        context.formula_api_path("testball"),
        format!(
            r#"{{
  "name": "testball",
  "full_name": "testball",
  "tap": "homebrew/core",
  "versions": {{
    "stable": "1.0"
  }},
  "revision": 0,
  "post_install_defined": false,
  "dependencies": [],
  "build_dependencies": [],
  "recommended_dependencies": [],
  "optional_dependencies": [],
  "uses_from_macos": [],
  "bottle": {{
    "stable": {{
      "rebuild": 0,
      "files": {{
        "x86_64_linux": {{
          "url": "file://{}",
          "sha256": "{}",
          "cellar": ":any_skip_relocation"
        }}
      }}
    }}
  }}
}}"#,
            bottle_source.display(),
            bottle_sha256,
        ),
    )
    .unwrap();

    let output = context
        .rust_command()
        .args(["install", "testball"])
        .output()
        .unwrap();

    assert!(!output.status.success(), "{output:?}");
    assert!(!context.cellar.join("testball").exists());
    assert!(!context.cellar.join("wrongball").exists());
}

#[test]
fn install_delegates_to_ruby_with_a_reason_when_post_install_is_defined() {
    let context = TestContext::new();

    fs::create_dir_all(context.formula_api_path("testball").parent().unwrap()).unwrap();
    fs::write(
        context.formula_api_path("testball"),
        r#"{
  "name": "testball",
  "full_name": "testball",
  "tap": "homebrew/core",
  "versions": {
    "stable": "1.0"
  },
  "revision": 0,
  "post_install_defined": true,
  "dependencies": [],
  "build_dependencies": [],
  "recommended_dependencies": [],
  "optional_dependencies": [],
  "uses_from_macos": [],
  "bottle": {
    "stable": {
      "rebuild": 0,
      "files": {
        "x86_64_linux": {
          "url": "file:///tmp/testball--1.0.x86_64_linux.bottle.tar.gz",
          "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
          "cellar": ":any_skip_relocation"
        }
      }
    }
  }
}"#,
    )
    .unwrap();

    let output = context
        .rust_command()
        .args(["install", "testball"])
        .output()
        .unwrap();
    let stderr = String::from_utf8(output.stderr.clone()).unwrap();

    assert!(
        stderr
            .contains("Warning: brew-rs is handing install back to the Ruby backend: `testball` defines `post_install`."),
        "{output:?}"
    );
}

fn new_tempdir() -> TestDir {
    let root = repo_root().join("tmp");
    fs::create_dir_all(&root).unwrap();
    TestDir::new_in(&root)
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../..")
        .canonicalize()
        .unwrap()
}

struct TestDir(PathBuf);

impl TestDir {
    fn new_in(root: &Path) -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let path = root.join(format!(
            "brew-rs-cli-{}-{}-{}",
            process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

fn sha256_hex(input: impl AsRef<[u8]>) -> String {
    use sha2::Digest;

    sha2::Sha256::digest(input.as_ref())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
