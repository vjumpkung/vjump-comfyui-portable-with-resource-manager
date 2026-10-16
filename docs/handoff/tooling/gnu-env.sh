# Local GNU-toolchain verification environment for Windows machines without MSVC C++ tools.
# Usage (Git Bash, from the repo root or a worktree): source docs/handoff/tooling/gnu-env.sh
# Requires: rustup toolchain 1.99.0-x86_64-pc-windows-gnu, MSYS2 UCRT64 gcc/windres at C:\msys64\ucrt64\bin.
_vj_tooling="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
export RUSTUP_TOOLCHAIN=1.99.0-x86_64-pc-windows-gnu CARGO_TARGET_DIR=target-gnu
export PATH="/c/msys64/ucrt64/bin:$PATH"
# Test runner: copies the 64-bit WebView2Loader.dll next to each test exe
# (Git Bash's /mingw64/bin ships a 32-bit copy that would otherwise be found first).
export CARGO_TARGET_X86_64_PC_WINDOWS_GNU_RUNNER="bash $_vj_tooling/gnu-runner.sh"
# Embed a Common Controls v6 manifest in every linked artifact (mingw's default manifest lacks it,
# so GUI-linked test executables fail with STATUS_ENTRYPOINT_NOT_FOUND).
if [ ! -f "$_vj_tooling/cc6.o" ]; then
  (cd "$_vj_tooling" && windres --input cc6.rc --output cc6.o --output-format=coff)
fi
export CARGO_TARGET_X86_64_PC_WINDOWS_GNU_RUSTFLAGS="-C link-arg=$(cygpath -m "$_vj_tooling/cc6.o")"
# Smaller debug info: keeps line tables for backtraces, cuts link memory and binary size.
export CARGO_PROFILE_DEV_DEBUG=line-tables-only
