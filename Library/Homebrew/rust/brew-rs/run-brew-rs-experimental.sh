#!/bin/bash
set -euo pipefail

if [[ -z "${HOMEBREW_DEVELOPER:-}" ]]
then
  echo "Error: HOMEBREW_DEVELOPER must already be set." >&2
  exit 1
fi

script_dir="$(
  cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P
)"
repository_root="$(
  cd -- "${script_dir}/../../../.." && pwd -P
)"
brew_file="${repository_root}/bin/brew"
vendor_binary="${repository_root}/Library/Homebrew/vendor/brew-rs/bin/brew-rs"

if [[ ! -x "${repository_root}/Library/Homebrew/vendor/brew-rs/brew-rs" ||
      ! -x "${vendor_binary}" ||
   "${script_dir}/Cargo.toml" -nt "${vendor_binary}" ||
   "${script_dir}/Cargo.lock" -nt "${vendor_binary}" ||
      -n "$(find "${script_dir}/src" -type f -newer "${vendor_binary}" -print -quit)" ]]
then
  HOMEBREW_EXPERIMENTAL_RUST_FRONTEND=1 \
    "${brew_file}" vendor-install brew-rs
fi

case "${1:-}" in
  fetch | search)
    cache_root="${HOMEBREW_CACHE:-$("${brew_file}" --cache)}"
    names_path="${cache_root}/api/formula_names.txt"
    cask_names_path="${cache_root}/api/cask_names.txt"
    formula_api_path="${cache_root}/api/formula.jws.json"

    if [[ ! -f "${names_path}" ||
          ! -f "${cask_names_path}" ||
          ("${1}" == "fetch" && ! -f "${formula_api_path}") ]]
    then
      HOMEBREW_EXPERIMENTAL_RUST_FRONTEND=1 \
        "${brew_file}" update
    fi
    ;;
  *) ;;
esac

exec env \
  HOMEBREW_EXPERIMENTAL_RUST_FRONTEND=1 \
  "${brew_file}" "$@"
