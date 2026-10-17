#!/bin/sh
# Install a pre-commit hook that refuses commits containing proprietary game files.
set -e
root=$(git rev-parse --show-toplevel)
hook="$root/.git/hooks/pre-commit"
cat > "$hook" <<'EOF'
#!/bin/sh
exec python3 "$(git rev-parse --show-toplevel)/scripts/check_no_proprietary.py" --staged
EOF
chmod +x "$hook"
echo "installed $hook"
