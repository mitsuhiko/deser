"""Hooks input capturing into the git config parsers of gcfg and gitoxide.

Every input the patched parsers read is appended to the JSON lines file
named by the INI_CAPTURE environment variable as {"test": ..., "input": ...}
(the input base64 encoded for gcfg, hex encoded for gitoxide).

Usage: python patch.py gcfg <gcfg-src>
       python patch.py gitoxide <gitoxide-src>
"""

import os
import shutil
import sys

HERE = os.path.dirname(os.path.abspath(__file__))

GITOXIDE_HOOK = r"""
// Added by scripts/update-ini-test-data.sh to capture the parsed inputs.
fn capture_input(input: &[u8]) {
    use std::io::Write;
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let Some(path) = std::env::var_os("INI_CAPTURE") else {
        return;
    };
    let thread = std::thread::current();
    let test: String = thread
        .name()
        .unwrap_or("")
        .chars()
        .flat_map(|c| match c {
            '"' | '\\' => vec!['\\', c],
            c if (c as u32) < 0x20 => vec![],
            c => vec![c],
        })
        .collect();
    let hex: String = input.iter().map(|b| format!("{b:02x}")).collect();
    // One write per record so that records of concurrent writers do not mix.
    let line = format!("{{\"test\":\"{test}\",\"input_hex\":\"{hex}\"}}\n");
    let _guard = LOCK.lock().unwrap();
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .unwrap();
    file.write_all(line.as_bytes()).unwrap();
}
"""


def patch(path, old, new):
    with open(path, encoding="utf-8") as f:
        source = f.read()
    if source.count(old) != 1:
        sys.exit(f"{path}: expected exactly one {old!r}")
    with open(path, "w", encoding="utf-8") as f:
        f.write(source.replace(old, new))


def patch_gcfg(src):
    shutil.copy(os.path.join(HERE, "capture.go"), os.path.join(src, "zz_capture.go"))
    patch(
        os.path.join(src, "read.go"),
        "fset *token.FileSet, file *token.File, src []byte) error {\n",
        "fset *token.FileSet, file *token.File, src []byte) error {\n"
        "\tcaptureInput(src)\n",
    )


def patch_gitoxide(src):
    path = os.path.join(src, "gix-config", "src", "parse", "from_bytes", "mod.rs")
    old = "pub(crate) fn from_bytes(mut input: &[u8], dispatch: &mut dyn FnMut(Event)) -> Result<(), Error> {\n"
    patch(path, old, old + "    capture_input(input);\n")
    with open(path, "a", encoding="utf-8") as f:
        f.write(GITOXIDE_HOOK)


{"gcfg": patch_gcfg, "gitoxide": patch_gitoxide}[sys.argv[1]](sys.argv[2])
