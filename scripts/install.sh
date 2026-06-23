#!/usr/bin/env sh
set -eu

show_help() {
  cat <<'EOF'
KDS Unix installer

Usage:
  ./scripts/install.sh [--dry-run] [--help]

Behavior:
  - builds KDS from this repository
  - installs kds to $HOME/.local/bin
  - does not silently edit PATH
  - does not modify Codex config
  - shell hooks are Windows PowerShell-only in V1
EOF
}

dry_run=0
for arg in "$@"; do
  case "$arg" in
    --dry-run) dry_run=1 ;;
    --help|-h) show_help; exit 0 ;;
    *) echo "unknown argument: $arg" >&2; exit 2 ;;
  esac
done

repo=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
install_dir="$HOME/.local/bin"
target="$install_dir/kds"
built="$repo/target/release/kds"

echo "KDS install plan"
echo "Repository: $repo"
echo "Install directory: $install_dir"
echo "Binary: $target"

if [ "$dry_run" -eq 1 ]; then
  echo "Dry run: no binary copy, no hook/profile edit, no Codex config edit, no PATH edit."
  exit 0
fi

(cd "$repo" && cargo build --release)
mkdir -p "$install_dir"
cp "$built" "$target"
echo "Wrote: $target"

case ":$PATH:" in
  *":$install_dir:"*) ;;
  *) echo "PATH note: $install_dir is not in PATH. Add it manually if kds is not found." ;;
esac

echo "Verification:"
echo "  kds --version"
echo "  kds gain"
echo "  kds doctor"
"$target" --version
