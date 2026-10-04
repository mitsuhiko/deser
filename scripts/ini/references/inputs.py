"""Lists the INI inputs of the corpus that get reference results.

These are all inputs except the git config ones (they have git as their
reference) and except files that only describe expectations (.phpt, tap
snapshots, ...).  Paths are relative to the data directory, sorted.

Usage: python inputs.py <data-dir>
"""

import os
import sys

ROOT = sys.argv[1]

PATTERNS = [
    "suites/inih/**/*.ini",
    "suites/iniparser/**/*.ini",
    "suites/editorconfig/parser/*.in",
    "suites/cpython-configparser/configdata/*",
    "suites/cpython-configparser/captured/*.ini",
    "suites/npm-ini/**/*.ini",
    "suites/js-ini-parser/**/*.ini",
    "suites/php/**/*.ini",
    "suites/php/**/*.data",
    "suites/go-ini/testdata/*.ini",
    "suites/ini4j/**/*.ini",
    "suites/simpleini/**/*.ini",
    "suites/dotnet-ini-parser/**/*.ini",
]
NOT_INPUTS = {"LICENSE", "LICENSE.txt", "LICENSE.TXT", "LICENSE-APACHE", "LICENSE-MIT",
              "LICENSE-MIT.txt", "LICENSES.txt", "COPYRIGHT", "SOURCE"}


def main():
    from glob import glob

    paths = set()
    for pattern in PATTERNS:
        paths.update(glob(pattern, root_dir=ROOT, recursive=True))
    for path in glob("real-world/**/*", root_dir=ROOT, recursive=True, include_hidden=True):
        name = os.path.basename(path)
        full = os.path.join(ROOT, path)
        if (
            os.path.isfile(full)
            and name not in NOT_INPUTS
            and not name.endswith(".git.json")
            and not os.path.exists(full + ".git.json")
        ):
            paths.add(path)
    for path in sorted(paths):
        print(path)


main()
