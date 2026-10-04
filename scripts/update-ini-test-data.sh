#!/usr/bin/env bash
# Vendors the INI test corpus into deser-ini/tests/data.
#
# INI has no specification and no shared test suite, so the corpus is put
# together from the test data of INI parsers and from real world files:
#
# * suites/: the test inputs (and where they are files, the expectations)
#   of INI parsers in various languages.  Every parser implements its own
#   dialect, see the README in the data directory.
#
#   cpython-configparser is special: the inputs of CPython's test suite are
#   inline strings, so they are captured by running the tests with
#   scripts/ini/capture-configparser.py, which also records how a fresh
#   parser with the same options reads them.  This needs uv.
#
# * suites/ for git config: git's own tests are GPL licensed, so the inputs
#   come from the test suites of other git config parsers (gitoxide, gcfg,
#   go-git, dulwich, JGit and isomorphic-git).  Running suites are patched to
#   record every input they parse (scripts/ini/git-config/patch.py,
#   capture-dulwich.py), JGit and isomorphic-git inputs are extracted from
#   string literals (extract-literals.py).  git itself is the reference:
#   scripts/ini/git-config/baseline.py stores how the pinned git reads every
#   input.  This needs docker, go and cargo.
#
# * real-world/: configuration files of well known projects in various INI
#   dialects (php.ini, tox.ini, .editorconfig, systemd units, desktop
#   entries, Windows INF files, gitconfig, ...).
#
# * references/: how inih, Python's configparser and PHP's parse_ini_string
#   (each in a few configurations) read every input that is not git config,
#   one file per input to compare the dialects.  See scripts/ini/references.
#
# Only sources under permissive licenses are vendored.  Every directory has
# a SOURCE file with the pinned origin and a copy of the license.
#
# To update, change the pinned commits below and re-run the script.
set -euo pipefail

INIH_COMMIT=2bbdec4a366c8c39746ee0982e7ca0febbb044b6
INIPARSER_COMMIT=7e2959bbb629883dfaf789eb31a27e232fc19cdb
EDITORCONFIG_COMMIT=895b3a65d0d823dbd0acf2bc402376381995d1b1
# Has to match CPYTHON_VERSION, the tests are run with that interpreter.
CPYTHON_VERSION=3.14.3
CPYTHON_COMMIT=v$CPYTHON_VERSION
NPM_INI_COMMIT=3c96c74fd42584bd655e17a4e63e2ef0a3b406ee
GO_INI_COMMIT=e2db55b0e088fa4ee0c128aa4ade263cdc1d7f08
INI4J_COMMIT=f987e6fe5f64e7d1044e343706deca56d55de89a
SIMPLEINI_COMMIT=fd6db69efc40a687bf4ef81486b54066e34992dd
DOTNET_INI_PARSER_COMMIT=d300241a75bec4dcabc0751c8690f3b12b3eaf43
JS_INI_PARSER_COMMIT=be39d624ab3fc01a0bd2bb2a4ed990f749c2d120
PHP_SRC_COMMIT=fab508fe1ca8e89d5ae16bf1cac31d07576462ee
GITEA_COMMIT=f65e01226a1135fe159dcde2ae5d8af5630d7ead
PGBOUNCER_COMMIT=7d38761c8f6c757238fde9f942cf9fe0cd272ae3
SUPERVISOR_COMMIT=abc60468ea4b78c446cf3a194f6fccb83d90f670
PYTEST_COMMIT=2887015cade4757385308e7a7d8083557fc637e2
DOTNET_RUNTIME_COMMIT=6f1d9331b9b477df73982a0fabedefe27f36d8a3
POSTGRES_COMMIT=e5d25959cf8761c7dbdd7cfbb91338e94a40e8e2
MOBY_COMMIT=c7b76b939576290b5daa5b3671fff06fcaeb6d2e
ALACRITTY_COMMIT=d692748d3f61253ebe9f5094320120d22f6a046f
RUST_COMMIT=db8f076d2619ce2585b0380dda06e8da25a40da4
ALEMBIC_COMMIT=b42ebe1ff576e02b8bfc9ef1c3c3a5fd1ac41bf4
MYPY_COMMIT=eb8cc4d05fde5c12194d929ace755807db4a3748
WINDOWS_DRIVER_SAMPLES_COMMIT=2dc3fd3a0cc84a2933f2194e7ec0871584979071
PHP_IMAGE=php:8.5.11-cli@sha256:19642e172d3a542225225e202ddc2c11f67bdcbddf147b676c49338609b9290f
# git config
GIT_IMAGE=alpine/git:v2.54.0@sha256:832b1cd1a271509f3d5272a1a62d4cb2ab1a53426ebde1f6c2cb7349f907dc6f
GITOXIDE_COMMIT=8f1280faf8dcabcfd967ee004a466f8b473b9b4f
# The gcfg version is the one go-git depends on.
GO_GIT_COMMIT=fedc50f4303a11413dbc3561963df6e3a93950b9
DULWICH_COMMIT=86e7b46d4347ee3f54c9be84a2ac7d27f562a731
JGIT_COMMIT=f0260542d4b1ce2f40a57df74addbef026e7cee7
ISOMORPHIC_GIT_COMMIT=431453fbe9350537b404769204b92f479e427c27
MATHIASBYNENS_DOTFILES_COMMIT=b7c7894e7bb2de5d60bfb9a2f5e46d01a61300ea
THOUGHTBOT_DOTFILES_COMMIT=939a27005b2dd4ac1556e104fc91122f3aea0a6c
GHOSTTY_COMMIT=befcdfd2c3a1cb24d9ec886e93c95b2b5daa7028

HERE="$(cd "$(dirname "$0")" && pwd)"
DATA="$(dirname "$HERE")/deser-ini/tests/data"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

for tool in uv docker go cargo cc; do
  if ! command -v $tool > /dev/null; then
    echo "$tool is required" >&2
    exit 1
  fi
done

# fetch_archive <name> <url>: unpacks an archive into $TMP/<name>.
fetch_archive() {
  mkdir -p "$TMP/$1"
  curl -fsSL "$2" | tar -xz -C "$TMP/$1" --strip-components=1
}

# github_archive <name> <repo> <commit>
github_archive() {
  fetch_archive "$1" "https://github.com/$2/archive/$3.tar.gz"
}

# copy <from> <to> <path>...: copies paths (relative to <from>) to <to>,
# keeping the directory structure.
copy() {
  local from="$1" to="$2"
  shift 2
  for path in "$@"; do
    mkdir -p "$to/$(dirname "$path")"
    cp "$from/$path" "$to/$path"
  done
}

# raw <repo> <commit> <path> <dest>: downloads a single file from GitHub.
raw() {
  mkdir -p "$(dirname "$4")"
  curl -fsSL "https://raw.githubusercontent.com/$1/$2/$3" -o "$4"
}

# source <dir> <url> <commit> [note]
source_file() {
  {
    echo "$2"
    echo "commit $3"
    if [ $# -gt 3 ]; then
      echo
      echo "$4"
    fi
  } > "$1/SOURCE"
}

rm -rf "$DATA/suites" "$DATA/real-world"
SUITES="$DATA/suites"
REAL="$DATA/real-world"

# -- suites ------------------------------------------------------------------

# inih (C): the baselines record the handler calls for every test file with
# different compile time options, see tests/unittest.sh.
github_archive inih benhoyt/inih "$INIH_COMMIT"
OUT="$SUITES/inih"
mkdir -p "$OUT"
(cd "$TMP/inih" && copy . "$OUT" tests/*.ini tests/baseline_*.txt \
  tests/unittest.c tests/unittest_string.c tests/unittest_alloc.c \
  tests/unittest.sh examples/test.ini LICENSE.txt)
source_file "$OUT" https://github.com/benhoyt/inih "$INIH_COMMIT"

# iniparser (C): good_ini/ and bad_ini/ have to load or fail to load, the
# expected values are in test/test_iniparser.c.
fetch_archive iniparser \
  "https://gitlab.com/iniparser/iniparser/-/archive/$INIPARSER_COMMIT/iniparser-$INIPARSER_COMMIT.tar.gz"
OUT="$SUITES/iniparser"
mkdir -p "$OUT"
(cd "$TMP/iniparser" && copy . "$OUT" $(cd "$TMP/iniparser" && find test/ressources -type f) \
  test/test_iniparser.c LICENSE)
source_file "$OUT" https://gitlab.com/iniparser/iniparser "$INIPARSER_COMMIT"

# editorconfig-core-test: the parser tests, the expectations are the
# regular expressions in parser/CMakeLists.txt.
github_archive editorconfig editorconfig/editorconfig-core-test "$EDITORCONFIG_COMMIT"
OUT="$SUITES/editorconfig"
mkdir -p "$OUT"
(cd "$TMP/editorconfig" && copy . "$OUT" parser/*.in parser/CMakeLists.txt LICENSE.txt)
source_file "$OUT" https://github.com/editorconfig/editorconfig-core-test "$EDITORCONFIG_COMMIT"

# CPython's configparser: the data files of the test suite and all inputs
# the test suite parses, captured together with the parse results.
mkdir -p "$TMP/cpython"
curl -fsSL "https://github.com/python/cpython/archive/$CPYTHON_COMMIT.tar.gz" \
  | tar -xz -C "$TMP/cpython" --strip-components=1 \
    "cpython-$CPYTHON_VERSION/Lib/test" "cpython-$CPYTHON_VERSION/Lib/idlelib" \
    "cpython-$CPYTHON_VERSION/LICENSE"
OUT="$SUITES/cpython-configparser"
mkdir -p "$OUT"
(cd "$TMP/cpython/Lib/test" && copy . "$OUT" configdata/cfgparser.1 \
  configdata/cfgparser.2 configdata/cfgparser.3)
uv run -q --no-project --python "$CPYTHON_VERSION" \
  python "$HERE/ini/capture-configparser.py" "$TMP/cpython" "$OUT/captured"
cp "$TMP/cpython/LICENSE" "$OUT/LICENSE"
source_file "$OUT" https://github.com/python/cpython "$CPYTHON_COMMIT" \
  "captured/ is generated by scripts/ini/capture-configparser.py with Python $CPYTHON_VERSION"

# npm's ini (JavaScript): the expectations are tap snapshots.
github_archive npm-ini npm/ini "$NPM_INI_COMMIT"
OUT="$SUITES/npm-ini"
mkdir -p "$OUT"
(cd "$TMP/npm-ini" && copy . "$OUT" test/fixtures/*.ini tap-snapshots/test/*.cjs LICENSE)
source_file "$OUT" https://github.com/npm/ini "$NPM_INI_COMMIT"

# go-ini (Go): the expectations are in the Go tests.
github_archive go-ini go-ini/ini "$GO_INI_COMMIT"
OUT="$SUITES/go-ini"
mkdir -p "$OUT"
(cd "$TMP/go-ini" && copy . "$OUT" testdata/* LICENSE)
source_file "$OUT" https://github.com/go-ini/ini "$GO_INI_COMMIT"

# ini4j (Java): encodings and the samples of the documentation.
github_archive ini4j ini4j/ini4j "$INI4J_COMMIT"
OUT="$SUITES/ini4j"
mkdir -p "$OUT"
(cd "$TMP/ini4j" && copy . "$OUT" src/test/resources/org/ini4j/spi/*.ini \
  src/test/resources/org/ini4j/addon/*.ini src/test/java/org/ini4j/sample/*.ini \
  LICENSE.txt)
source_file "$OUT" https://github.com/ini4j/ini4j "$INI4J_COMMIT"

# SimpleIni (C++)
github_archive simpleini brofield/simpleini "$SIMPLEINI_COMMIT"
OUT="$SUITES/simpleini"
mkdir -p "$OUT"
(cd "$TMP/simpleini" && copy . "$OUT" tests/*.ini tests/data/* LICENCE.txt)
source_file "$OUT" https://github.com/brofield/simpleini "$SIMPLEINI_COMMIT"

# ini-parser (.NET)
github_archive dotnet-ini-parser rickyah/ini-parser "$DOTNET_INI_PARSER_COMMIT"
OUT="$SUITES/dotnet-ini-parser"
mkdir -p "$OUT"
(cd "$TMP/dotnet-ini-parser" && copy . "$OUT" src/IniParser.Tests/*.ini \
  src/IniParser.Example/TestIniFile.ini LICENSE)
source_file "$OUT" https://github.com/rickyah/ini-parser "$DOTNET_INI_PARSER_COMMIT"

# ini-parser (JavaScript): every fixture has the expected result in index.js.
github_archive js-ini-parser jednano/ini-parser "$JS_INI_PARSER_COMMIT"
OUT="$SUITES/js-ini-parser"
mkdir -p "$OUT"
(cd "$TMP/js-ini-parser" && copy . "$OUT" $(cd "$TMP/js-ini-parser" && find src/fixtures -type f) LICENSE)
source_file "$OUT" https://github.com/jednano/ini-parser "$JS_INI_PARSER_COMMIT"

# PHP's parse_ini_file/parse_ini_string: the .phpt files have the inputs
# (inline or as the referenced files) and the expected var_dump output.
OUT="$SUITES/php"
for path in \
  Zend/tests/bug74603.ini \
  Zend/tests/bug74603.phpt \
  ext/standard/tests/file/parse_ini_file.phpt \
  ext/standard/tests/file/parse_ini_file_error.phpt \
  ext/standard/tests/file/parse_ini_file_variation1.phpt \
  ext/standard/tests/file/parse_ini_file_variation2.phpt \
  ext/standard/tests/file/parse_ini_file_variation3.phpt \
  ext/standard/tests/file/parse_ini_file_variation6.phpt \
  ext/standard/tests/general_functions/bug49692.ini \
  ext/standard/tests/general_functions/bug49692.phpt \
  ext/standard/tests/general_functions/parse_ini_basic.data \
  ext/standard/tests/general_functions/parse_ini_basic.phpt \
  ext/standard/tests/general_functions/parse_ini_booleans.data \
  ext/standard/tests/general_functions/parse_ini_booleans.phpt \
  ext/standard/tests/general_functions/parse_ini_file.phpt \
  ext/standard/tests/general_functions/parse_ini_numeric_entry_name.phpt \
  ext/standard/tests/general_functions/parse_ini_string_001.phpt \
  ext/standard/tests/general_functions/parse_ini_string_002.phpt \
  ext/standard/tests/general_functions/parse_ini_string_003.phpt \
  ext/standard/tests/general_functions/parse_ini_string_bug76068.phpt \
  ext/standard/tests/general_functions/parse_ini_string_error.phpt \
  LICENSE
do
  raw php/php-src "$PHP_SRC_COMMIT" "$path" "$OUT/$path"
done
# browscap has unusual section names, the first lines are enough for them.
BROWSCAP=ext/standard/tests/misc/browscap_lite_2016_12_06.ini
raw php/php-src "$PHP_SRC_COMMIT" "$BROWSCAP" "$TMP/browscap.ini"
mkdir -p "$OUT/$(dirname "$BROWSCAP")"
head -n 300 "$TMP/browscap.ini" > "$OUT/$BROWSCAP"
source_file "$OUT" https://github.com/php/php-src "$PHP_SRC_COMMIT" \
  "$BROWSCAP is cut after 300 lines."

# -- real world files --------------------------------------------------------

# real_world <name> <repo> <commit> <license> <path>...
real_world() {
  local name="$1" repo="$2" commit="$3" license="$4"
  shift 4
  local out="$REAL/$name"
  for path in "$@" "$license"; do
    raw "$repo" "$commit" "$path" "$out/$path"
  done
  source_file "$out" "https://github.com/$repo" "$commit"
}

real_world php php/php-src "$PHP_SRC_COMMIT" LICENSE \
  php.ini-development php.ini-production sapi/fpm/php-fpm.conf.in sapi/fpm/www.conf.in
real_world gitea go-gitea/gitea "$GITEA_COMMIT" LICENSE custom/conf/app.example.ini
real_world pgbouncer pgbouncer/pgbouncer "$PGBOUNCER_COMMIT" COPYRIGHT etc/pgbouncer.ini
real_world supervisor Supervisor/supervisor "$SUPERVISOR_COMMIT" LICENSES.txt \
  supervisor/skel/sample.conf
real_world pytest pytest-dev/pytest "$PYTEST_COMMIT" LICENSE tox.ini
real_world dotnet-runtime dotnet/runtime "$DOTNET_RUNTIME_COMMIT" LICENSE.TXT .editorconfig
real_world postgres postgres/postgres "$POSTGRES_COMMIT" COPYRIGHT \
  src/interfaces/libpq/pg_service.conf.sample
real_world moby moby/moby "$MOBY_COMMIT" LICENSE \
  contrib/init/systemd/docker.service contrib/init/systemd/docker.socket
real_world alacritty alacritty/alacritty "$ALACRITTY_COMMIT" LICENSE-APACHE \
  extra/linux/Alacritty.desktop
real_world rust rust-lang/rust "$RUST_COMMIT" LICENSE-MIT .gitmodules
real_world alembic sqlalchemy/alembic "$ALEMBIC_COMMIT" LICENSE \
  alembic/templates/generic/alembic.ini.mako
real_world mypy python/mypy "$MYPY_COMMIT" LICENSE mypy_self_check.ini
real_world windows-driver-samples microsoft/Windows-driver-samples \
  "$WINDOWS_DRIVER_SAMPLES_COMMIT" LICENSE \
  filesys/miniFilter/avscan/avscan.inf avstream/avshws/avshws.inx

# IDLE's default configuration, read with configparser.
OUT="$REAL/cpython-idlelib"
mkdir -p "$OUT"
(cd "$TMP/cpython" && copy . "$OUT" Lib/idlelib/config-extensions.def \
  Lib/idlelib/config-highlight.def Lib/idlelib/config-keys.def \
  Lib/idlelib/config-main.def LICENSE)
source_file "$OUT" https://github.com/python/cpython "$CPYTHON_COMMIT"

real_world dotfiles-mathiasbynens mathiasbynens/dotfiles \
  "$MATHIASBYNENS_DOTFILES_COMMIT" LICENSE-MIT.txt .gitconfig
real_world dotfiles-thoughtbot thoughtbot/dotfiles \
  "$THOUGHTBOT_DOTFILES_COMMIT" LICENSE gitconfig
real_world ghostty ghostty-org/ghostty "$GHOSTTY_COMMIT" LICENSE .gitmodules

# -- git config --------------------------------------------------------------

CAPTURE="$TMP/capture"
mkdir -p "$CAPTURE"

# gitoxide: gix-config's tests with the parser patched.
github_archive gitoxide GitoxideLabs/gitoxide "$GITOXIDE_COMMIT"
python3 "$HERE/ini/git-config/patch.py" gitoxide "$TMP/gitoxide"
(cd "$TMP/gitoxide" && INI_CAPTURE="$CAPTURE/gitoxide.jsonl" \
  CARGO_TARGET_DIR="$TMP/gitoxide-target" cargo test -q -p gix-config > /dev/null)

# gcfg and go-git: go-git reads git config with gcfg, so the patched gcfg
# captures the inputs of its own tests and, swapped in, the ones of go-git.
github_archive go-git go-git/go-git "$GO_GIT_COMMIT"
GCFG_VERSION="$(cd "$TMP/go-git" && go list -m -f '{{.Version}}' github.com/go-git/gcfg/v2)"
fetch_archive gcfg "https://github.com/go-git/gcfg/archive/refs/tags/$GCFG_VERSION.tar.gz"
python3 "$HERE/ini/git-config/patch.py" gcfg "$TMP/gcfg"
(cd "$TMP/gcfg" && INI_CAPTURE="$CAPTURE/gcfg.jsonl" go test -count=1 ./... > /dev/null)
(cd "$TMP/go-git" && go mod edit -replace "github.com/go-git/gcfg/v2=$TMP/gcfg" \
  && INI_CAPTURE="$CAPTURE/go-git.jsonl" go test -count=1 ./config/ ./plumbing/format/config/ > /dev/null)

# dulwich: tests/test_config.py with ConfigFile.from_file patched.
github_archive dulwich jelmer/dulwich "$DULWICH_COMMIT"
uv run -q --no-project --python "$CPYTHON_VERSION" \
  python "$HERE/ini/git-config/capture-dulwich.py" "$TMP/dulwich" "$CAPTURE/dulwich.jsonl"

# JGit and isomorphic-git: string literals.
github_archive jgit eclipse-jgit/jgit "$JGIT_COMMIT"
python3 "$HERE/ini/git-config/extract-literals.py" java "$CAPTURE/jgit.jsonl" \
  $(cd "$TMP/jgit" && grep -rl 'fromText(\|Config parse(' --include='*Test.java' \
    "$TMP/jgit/org.eclipse.jgit.test/tst" | sort)
github_archive isomorphic-git isomorphic-git/isomorphic-git "$ISOMORPHIC_GIT_COMMIT"
python3 "$HERE/ini/git-config/extract-literals.py" js "$CAPTURE/isomorphic-git.jsonl" \
  "$TMP/isomorphic-git/__tests__/test-GitConfig.js"

python3 "$HERE/ini/git-config/baseline.py" "$GIT_IMAGE" "$DATA" \
  "suites/gitoxide/captured=$CAPTURE/gitoxide.jsonl" \
  "suites/gcfg/captured=$CAPTURE/gcfg.jsonl" \
  "suites/go-git/captured=$CAPTURE/go-git.jsonl" \
  "suites/dulwich/captured=$CAPTURE/dulwich.jsonl" \
  "suites/jgit/captured=$CAPTURE/jgit.jsonl" \
  "suites/isomorphic-git/captured=$CAPTURE/isomorphic-git.jsonl" \
  --extra \
  "$REAL/dotfiles-mathiasbynens/.gitconfig" \
  "$REAL/dotfiles-thoughtbot/gitconfig" \
  "$REAL/ghostty/.gitmodules" \
  "$REAL/rust/.gitmodules"

GIT_NOTE="captured/ has the inputs of the test suite (see scripts/ini/git-config),
the .json files how git reads them ($GIT_IMAGE)."
cp "$TMP/gitoxide/gix-config/LICENSE-MIT" "$TMP/gitoxide/gix-config/LICENSE-APACHE" "$SUITES/gitoxide/"
source_file "$SUITES/gitoxide" https://github.com/GitoxideLabs/gitoxide "$GITOXIDE_COMMIT" "$GIT_NOTE"
cp "$TMP/gcfg/LICENSE" "$SUITES/gcfg/"
source_file "$SUITES/gcfg" https://github.com/go-git/gcfg "$GCFG_VERSION" "$GIT_NOTE"
cp "$TMP/go-git/LICENSE" "$SUITES/go-git/"
source_file "$SUITES/go-git" https://github.com/go-git/go-git "$GO_GIT_COMMIT" "$GIT_NOTE"
cp "$TMP/dulwich/COPYING" "$SUITES/dulwich/"
source_file "$SUITES/dulwich" https://github.com/jelmer/dulwich "$DULWICH_COMMIT" \
  "$GIT_NOTE
Used under the Apache License 2.0 (dulwich is dual licensed)."
cp "$TMP/jgit/LICENSE" "$SUITES/jgit/"
source_file "$SUITES/jgit" https://github.com/eclipse-jgit/jgit "$JGIT_COMMIT" "$GIT_NOTE"
cp "$TMP/isomorphic-git/LICENSE.md" "$SUITES/isomorphic-git/"
source_file "$SUITES/isomorphic-git" https://github.com/isomorphic-git/isomorphic-git \
  "$ISOMORPHIC_GIT_COMMIT" "$GIT_NOTE"

# -- references --------------------------------------------------------------

REFS="$TMP/references"
mkdir -p "$REFS"
uv run -q --no-project --python "$CPYTHON_VERSION" \
  python "$HERE/ini/references/inputs.py" "$DATA" > "$REFS/inputs.txt"

# inih, with the limits on the length of lines, sections and names lifted
# and the handler called for every section (also empty ones).
INIH_FLAGS="-DINI_HANDLER_LINENO=1 -DINI_CALL_HANDLER_ON_NEW_SECTION=1 \
  -DINI_USE_STACK=0 -DINI_ALLOW_REALLOC=1 -DINI_MAX_LINE=1048576 \
  -DINI_MAX_SECTION=4096 -DINI_MAX_NAME=4096"
INIH_ARGS=()
for variant in \
  default: \
  no_multiline:-DINI_ALLOW_MULTILINE=0 \
  no_inline_comments:-DINI_ALLOW_INLINE_COMMENTS=0 \
  allow_no_value:-DINI_ALLOW_NO_VALUE=1
do
  name="${variant%%:*}"
  # shellcheck disable=SC2086
  cc -O1 $INIH_FLAGS ${variant#*:} -I"$TMP/inih" "$TMP/inih/ini.c" \
    "$HERE/ini/references/inih-driver.c" -o "$REFS/inih-$name"
  (cd "$DATA" && "$REFS/inih-$name" < "$REFS/inputs.txt" > "$REFS/inih-$name.jsonl")
  INIH_ARGS+=("inih:$name=$REFS/inih-$name.jsonl")
done

uv run -q --no-project --python "$CPYTHON_VERSION" \
  python "$HERE/ini/references/run-configparser.py" "$DATA" \
  < "$REFS/inputs.txt" > "$REFS/configparser.jsonl"

docker run --rm -i -v "$DATA:/data:ro" -v "$HERE/ini/references:/scripts:ro" \
  "$PHP_IMAGE" env -i /usr/local/bin/php /scripts/run-php.php /data \
  < "$REFS/inputs.txt" > "$REFS/php.jsonl"

python3 "$HERE/ini/references/write.py" "$DATA" "${INIH_ARGS[@]}" \
  "$REFS/configparser.jsonl" "$REFS/php.jsonl"
cat > "$DATA/references/SOURCE" <<EOF
inih: https://github.com/benhoyt/inih commit $INIH_COMMIT
configparser: Python $CPYTHON_VERSION
php: $PHP_IMAGE

Generated by scripts/update-ini-test-data.sh, see scripts/ini/references.
EOF

echo "Updated $DATA"
