#!/usr/bin/env bash
# Builds transcribe-server: transcribe.cpp at the pinned commit (Metal) plus our transcribe-server.cpp.
# Output: sidecar/transcribe-server/transcribe-server. First build compiles ggml (a few minutes).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")" && pwd)"
COMMIT="$(cat "$ROOT/PINNED_COMMIT")"
SRC="$ROOT/.transcribe.cpp"
REPO="https://github.com/handy-computer/transcribe.cpp"
JOBS="${OPENFLOW_BUILD_JOBS:-$(sysctl -n hw.perflevel0.physicalcpu 2>/dev/null || echo 4)}"

if [ ! -e "$SRC/.git" ]; then
  echo "Fetching transcribe.cpp @ ${COMMIT:0:12} …"
  rm -rf "$SRC"
  mkdir -p "$SRC"
  git -C "$SRC" init -q
  git -C "$SRC" remote add origin "$REPO"
  git -C "$SRC" fetch -q --depth 1 origin "$COMMIT"
  git -C "$SRC" checkout -q FETCH_HEAD
fi
# Never build a stale checkout: the source must be exactly the pinned commit.
ACTUAL="$(git -C "$SRC" rev-parse HEAD)"
if [[ "$ACTUAL" != "$COMMIT" ]]; then
  echo "transcribe.cpp checkout is $ACTUAL, expected $COMMIT; remove $SRC and rerun" >&2
  exit 1
fi

# Our server builds as an extra example target, which links the transcribe library and ggml/Metal.
mkdir -p "$SRC/examples/openflow-server"
cp "$ROOT/transcribe-server.cpp" "$ROOT/httplib.h" "$SRC/examples/openflow-server/"
cat > "$SRC/examples/openflow-server/CMakeLists.txt" <<'CM'
add_executable(transcribe-server transcribe-server.cpp)
target_include_directories(transcribe-server PRIVATE ${CMAKE_CURRENT_SOURCE_DIR})
find_package(Threads REQUIRED)
target_link_libraries(transcribe-server PRIVATE transcribe Threads::Threads)
CM
grep -q "add_subdirectory(openflow-server)" "$SRC/examples/CMakeLists.txt" ||
  echo "add_subdirectory(openflow-server)" >> "$SRC/examples/CMakeLists.txt"

echo "Building transcribe-server (Metal, $JOBS jobs) …"
cmake -S "$SRC" -B "$SRC/build" -DCMAKE_BUILD_TYPE=Release -DTRANSCRIBE_BUILD_EXAMPLES=ON -DBUILD_SHARED_LIBS=OFF >/dev/null
cmake --build "$SRC/build" --target transcribe-server -j "$JOBS" >/dev/null
BIN="$(find "$SRC/build" -type f -name transcribe-server -perm -u+x | head -1)"
cp "$BIN" "$ROOT/transcribe-server"
codesign --force --sign - "$ROOT/transcribe-server" 2>/dev/null || true
"$ROOT/transcribe-server" --selftest
echo "transcribe-server -> $ROOT/transcribe-server"
