#!/bin/bash
#
# Install what `cargo xtask shoot` needs to run the engine and photograph it.
#
# This engine is verified by looking at it — `.claude/CLAUDE.md` says so, and six
# bugs on record passed the whole test suite and were caught by running a demo.
# On a developer's machine "run it and look at it" is free. In a fresh cloud
# container there is no GPU, no X server, and no screenshot tool, and whatever
# gets installed by hand is lost when the container is reclaimed. This hook is
# what makes the Definition of Done reachable there.
#
# It is a no-op anywhere else: a local machine already has a real GPU and a real
# display, and should not have Xvfb and a software rasterizer installed behind
# its back.
set -euo pipefail

if [ "${CLAUDE_CODE_REMOTE:-}" != "true" ]; then
  exit 0
fi

PROJECT_DIR="${CLAUDE_PROJECT_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"

# `sudo` only if we are not already root — containers vary.
if [ "$(id -u)" -eq 0 ]; then
  SUDO=""
elif command -v sudo >/dev/null 2>&1; then
  SUDO="sudo"
else
  echo "session-start: not root and no sudo; skipping package install" >&2
  SUDO=""
fi

# --- System packages ---------------------------------------------------------
#
# What each one is for, because none of it is guessable from the name:
#
#   xvfb                  a virtual X server; the engine needs a window to exist
#                         before wgpu will hand it a surface.
#   mesa-vulkan-drivers   lavapipe, a software Vulkan implementation. Native
#                         builds request `Backends::PRIMARY`, which is Vulkan on
#                         Linux, so with no ICD installed there is no adapter at
#                         all and `create_surface` fails outright.
#   libxkbcommon-x11-0    winit dlopen()s this at startup and *panics* without
#                         it, before any of the above matters.
#   libegl1               the GL path's loader. Not needed for the Vulkan
#                         adapter, but it is what a WebGL2-parity check would
#                         want and it costs almost nothing.
#   imagemagick           `import` to grab the window, `compare` to diff two runs.
#   xdotool               synthetic pointer input, for scripted captures.
install_packages() {
  local pkgs=(
    xvfb
    mesa-vulkan-drivers
    libxkbcommon-x11-0
    libegl1
    imagemagick
    xdotool
  )

  local missing=()
  for pkg in "${pkgs[@]}"; do
    if ! dpkg -s "$pkg" >/dev/null 2>&1; then
      missing+=("$pkg")
    fi
  done

  if [ ${#missing[@]} -eq 0 ]; then
    echo "session-start: system packages already present"
    return 0
  fi

  echo "session-start: installing ${missing[*]}"
  # `update` first: a fresh container's index is stale enough that fetching a
  # package 404s on a version that has since been superseded. `|| true` because
  # a third-party PPA that the proxy refuses should not fail the whole hook —
  # the archives we actually need are on the main mirror.
  $SUDO apt-get update -qq || true
  DEBIAN_FRONTEND=noninteractive $SUDO apt-get install -y --no-install-recommends "${missing[@]}"
}

# --- wasm-bindgen CLI --------------------------------------------------------
#
# The CLI must match the `wasm-bindgen` *library* version pinned in Cargo.lock or
# `cargo xtask serve` fails with a schema mismatch. `.claude/CLAUDE.md` tells a
# human to keep those in step by hand; this reads the answer out of the lockfile
# instead, so it cannot drift.
#
# Also the slowest step here by a wide margin (it compiles), which is exactly why
# it is worth doing once and letting the container image cache it.
install_wasm_bindgen() {
  local want
  want="$(awk '/^name = "wasm-bindgen"$/ { getline; gsub(/version = |"/, ""); print; exit }' \
    "$PROJECT_DIR/Cargo.lock")"

  if [ -z "$want" ]; then
    echo "session-start: no wasm-bindgen in Cargo.lock; skipping CLI install" >&2
    return 0
  fi

  local have=""
  if command -v wasm-bindgen >/dev/null 2>&1; then
    have="$(wasm-bindgen --version 2>/dev/null | awk '{print $2}')"
  fi

  if [ "$have" = "$want" ]; then
    echo "session-start: wasm-bindgen-cli $want already installed"
    return 0
  fi

  echo "session-start: installing wasm-bindgen-cli $want (have: ${have:-none})"
  cargo install wasm-bindgen-cli --version "$want" --locked
}

install_packages
install_wasm_bindgen

# Warm the build cache so the first `cargo xtask shoot` of a session is not also
# a cold compile of wgpu. Failure here is not fatal — it is an optimisation, and
# a broken build should be reported by the agent's own `cargo build`, not by a
# hook that runs before anyone is watching.
echo "session-start: warming the build cache"
(cd "$PROJECT_DIR" && cargo build --examples) || \
  echo "session-start: warm-up build failed; continuing" >&2

echo "session-start: ready — try 'cargo xtask shoot triangle'"
