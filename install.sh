#!/bin/sh
# Installer for the DECLUTTER-INATOR — https://declutter.kunalbagaria.com
#
#     curl -fsSL https://declutter.kunalbagaria.com/install | sh
#
# It downloads one universal macOS binary, checks its SHA-256, puts it in
# ~/.local/bin and, if that folder isn't on your PATH yet, adds it to your
# shell's startup file. No sudo, nothing else on your system is touched.
#
# Settings (environment variables, e.g. `curl … | DECLUTTER_VERSION=0.1.0 sh`):
#     DECLUTTER_INSTALL_DIR=DIR    install somewhere else (default ~/.local/bin)
#     DECLUTTER_VERSION=X.Y.Z      install a specific version (default: latest)
#     DECLUTTER_NO_MODIFY_PATH=1   leave shell startup files alone
#
# Uninstall:  rm ~/.local/bin/declutterinator

set -eu

BASE_URL="${DECLUTTER_BASE_URL:-https://declutter.kunalbagaria.com}"

setup_colors() {
    if [ -t 1 ] && [ -z "${NO_COLOR:-}" ] && [ "${TERM:-}" != "dumb" ]; then
        PINK=$(printf '\033[38;5;199m') CYAN=$(printf '\033[38;5;51m') DIM=$(printf '\033[38;5;244m')
        BOLD=$(printf '\033[1m') RED=$(printf '\033[38;5;203m') RESET=$(printf '\033[0m')
    else
        PINK='' CYAN='' DIM='' BOLD='' RED='' RESET=''
    fi
}

say() { printf '  %s\n' "$*"; }
die() {
    printf '\n  %sCurses!%s %s\n\n' "$RED" "$RESET" "$*" >&2
    exit 1
}
need() { command -v "$1" >/dev/null 2>&1 || die "This installer needs '$1', which isn't on your PATH."; }
fetch() { curl --fail --silent --show-error --location --retry 2 "$1"; }

# Show $HOME as ~ in messages.
tildify() {
    # shellcheck disable=SC2088 # a literal ~ is the point
    case "$1" in
        "$HOME"/*) printf '~/%s' "${1#"$HOME"/}" ;;
        *) printf '%s' "$1" ;;
    esac
}

# Write the PATH line using $HOME when possible, so dotfiles stay portable.
portable() {
    # shellcheck disable=SC2016 # a literal $HOME is the point
    case "$1" in
        "$HOME"/*) printf '$HOME/%s' "${1#"$HOME"/}" ;;
        *) printf '%s' "$1" ;;
    esac
}

on_path() {
    case ":$PATH:" in
        *":$1:"*) return 0 ;;
        *) return 1 ;;
    esac
}

# Add $1 to PATH in the user's login-shell startup file. Sets PROFILE.
add_to_path() {
    dir_expr=$(portable "$1")
    case "$(basename "${SHELL:-sh}")" in
        zsh)
            PROFILE="${ZDOTDIR:-$HOME}/.zshrc"
            line="export PATH=\"$dir_expr:\$PATH\""
            ;;
        bash)
            PROFILE="$HOME/.bash_profile"
            line="export PATH=\"$dir_expr:\$PATH\""
            ;;
        fish)
            PROFILE="${XDG_CONFIG_HOME:-$HOME/.config}/fish/conf.d/declutterinator.fish"
            line="fish_add_path \"$dir_expr\""
            ;;
        *)
            PROFILE="$HOME/.profile"
            line="export PATH=\"$dir_expr:\$PATH\""
            ;;
    esac
    if [ -f "$PROFILE" ] && grep -Fq "$line" "$PROFILE"; then
        return 0
    fi
    mkdir -p "$(dirname "$PROFILE")"
    printf '\n# Added by the declutterinator installer\n%s\n' "$line" >>"$PROFILE"
}

main() {
    setup_colors

    [ "$(uname -s)" = "Darwin" ] || die "The DECLUTTER-INATOR only works on macOS, and this is $(uname -s)."
    os_version=$(sw_vers -productVersion)
    [ "${os_version%%.*}" -ge 11 ] || die "It needs macOS 11 or later, and this is macOS $os_version."
    for tool in curl tar shasum mktemp install; do need "$tool"; done

    version="${DECLUTTER_VERSION:-}"
    if [ -z "$version" ]; then
        version=$(fetch "$BASE_URL/releases/latest.txt") || die "Couldn't reach $BASE_URL. Check your connection and try again."
        version=$(printf '%s' "$version" | tr -d '[:space:]')
    fi
    version=${version#v}
    case "$version" in
        '' | *[!0-9.]*) die "The server sent an odd version number ('$version')." ;;
    esac

    name="declutterinator-$version-macos-universal"
    url="$BASE_URL/releases/v$version/$name.tar.gz"
    dir="${DECLUTTER_INSTALL_DIR:-$HOME/.local/bin}"
    target="$dir/declutterinator"

    previous=""
    if [ -x "$target" ]; then
        previous=$("$target" --version 2>/dev/null | awk '{print $2}') || previous=""
    fi

    tmp=$(mktemp -d "${TMPDIR:-/tmp}/declutterinator.XXXXXX")
    trap 'rm -rf "$tmp"' EXIT INT TERM

    printf '\n'
    say "${PINK}▓▒░${RESET} ${BOLD}DECLUTTER-INATOR${RESET} ${CYAN}░▒▓${RESET}  ${DIM}installer${RESET}"
    printf '\n'
    say "Downloading version $version ${DIM}(macOS universal)${RESET}"
    fetch "$url" >"$tmp/$name.tar.gz" || die "Couldn't download version $version. Check the version number and your connection."
    fetch "$url.sha256" >"$tmp/$name.tar.gz.sha256" || die "Couldn't download the checksum for $name."

    say "Checking the SHA-256 checksum"
    (cd "$tmp" && shasum -a 256 -c "$name.tar.gz.sha256" >/dev/null 2>&1) ||
        die "The checksum doesn't match, so the download is damaged or was tampered with. Nothing was installed."

    tar -xzf "$tmp/$name.tar.gz" -C "$tmp"
    [ -f "$tmp/$name/declutterinator" ] || die "The archive doesn't contain the declutterinator binary."
    "$tmp/$name/declutterinator" --version >/dev/null 2>&1 || die "The downloaded binary won't run on this Mac."

    mkdir -p "$dir" 2>/dev/null || die "Couldn't create $dir. Pick another folder with DECLUTTER_INSTALL_DIR=..."
    install -m 755 "$tmp/$name/declutterinator" "$target" 2>/dev/null ||
        die "Couldn't write to $dir. Pick another folder with DECLUTTER_INSTALL_DIR=..."

    if [ -n "$previous" ] && [ "$previous" != "$version" ]; then
        say "Upgraded $previous to $version in $(tildify "$dir")"
    else
        say "Installed to $(tildify "$target")"
    fi

    PROFILE=""
    if ! on_path "$dir" && [ -z "${DECLUTTER_NO_MODIFY_PATH:-}" ]; then
        add_to_path "$dir"
        say "Added $(tildify "$dir") to your PATH in $(tildify "$PROFILE")"
    fi

    found=$(command -v declutterinator 2>/dev/null || true)
    if [ -n "$found" ] && [ "$found" != "$target" ]; then
        say "${RED}Note:${RESET} another declutterinator at $found comes first on your PATH."
    fi

    printf '\n'
    if on_path "$dir"; then
        say "Behold. Run ${BOLD}declutterinator${RESET} to scan your disk."
    elif [ -n "$PROFILE" ]; then
        say "Behold. Open a new terminal and run ${BOLD}declutterinator${RESET},"
        say "or start right now with ${BOLD}$(tildify "$target")${RESET}"
    else
        say "Behold. Run ${BOLD}$(tildify "$target")${RESET} to scan your disk."
    fi
    say "${DIM}Nothing is removed until you confirm. Preview with: declutterinator clean --dry-run${RESET}"
    printf '\n'
}

main "$@"
