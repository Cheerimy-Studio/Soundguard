#!/usr/bin/env python3
"""Redirect the legacy CoTaskMem* imports from combase.dll to ole32.dll.

Windows 7 has no combase.dll. The windows-rs bindings used by wasapi 0.19 and
webview2-com 0.34 link CoTaskMemAlloc/CoTaskMemFree against combase.dll, which
makes the executable fail to load on Windows 7 with "combase.dll is missing".

Both functions exist in ole32.dll on Windows 7 and newer Windows versions, so
rewriting the import descriptor name is enough. The PE checksum is refreshed.
"""

from __future__ import annotations

import argparse
import pathlib
import sys

try:
    import pefile
except ImportError:
    sys.exit("pefile is required: python -m pip install pefile")


def patch(path: pathlib.Path) -> int:
    pe = pefile.PE(str(path))
    changed = 0
    try:
        for entry in pe.DIRECTORY_ENTRY_IMPORT:
            if entry.dll.lower() != b"combase.dll":
                continue
            allowed = {b"CoTaskMemAlloc", b"CoTaskMemFree", b"CoTaskMemRealloc"}
            unsupported = [item.name or f"ordinal:{item.ordinal}" for item in entry.imports if item.name not in allowed]
            if unsupported:
                raise RuntimeError(f"Refusing to redirect unsupported combase imports: {unsupported}")
            # "combase.dll\0" -> "ole32.dll\0" (the remaining byte is padding)
            pe.set_bytes_at_rva(entry.struct.Name, b"ole32.dll\x00\x00")
            changed += 1
        if changed:
            pe.OPTIONAL_HEADER.CheckSum = pe.generate_checksum()
            pe.write(str(path))
    finally:
        pe.close()
    return changed


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "binary",
        nargs="?",
        default=str(
            pathlib.Path(__file__).resolve().parent.parent
            / "target"
            / "release"
            / "soundguard-client.exe"
        ),
    )
    args = parser.parse_args()
    path = pathlib.Path(args.binary)
    if not path.is_file():
        sys.exit(f"binary not found: {path}")
    changed = patch(path)
    if changed == 0:
        print("No combase.dll import found; nothing to patch.")
    else:
        print(f"Patched {changed} import descriptor(s) to ole32.dll: {path}")


if __name__ == "__main__":
    main()
