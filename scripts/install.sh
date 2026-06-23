#!/usr/bin/env sh
set -eu

show_help() {
  cat <<'EOF'
KDS Unix installer

Usage:
  ./scripts/install.sh [--binary-only] [--dry-run] [--help]

Behavior:
  - Unix automatic shell hooks are not implemented in V1
  - refuses product-style install unless --binary-only is explicit
  - with --binary-only, builds KDS and installs kds to $HOME/.local/bin
  - does not silently edit PATH
  - does not modify Codex config
EOF
}

dry_run=0
binary_only=0
for arg in "$@"; do
  case "$arg" in
    --binary-only) binary_only=1 ;;
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
echo "Automatic hook: unavailable on Unix in V1"

if [ "$dry_run" -eq 1 ]; then
  echo "Dry run: no binary copy, no hook/profile edit, no Codex config edit, no PATH edit."
  exit 0
fi

if [ "$binary_only" -ne 1 ]; then
  echo "Refusing install: KDS install is automatic-hook-first, and Unix shell hooks are not implemented in V1." >&2
  echo "Use --binary-only only for development or explicit manual use without activation." >&2
  exit 2
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
