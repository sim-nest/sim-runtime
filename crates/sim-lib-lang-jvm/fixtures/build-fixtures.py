#!/usr/bin/env python3
"""Rebuild the frozen javac and independently hand-built baseline fixtures."""

import pathlib
import struct
import subprocess

ROOT = pathlib.Path(__file__).resolve().parent
(ROOT / "javac").mkdir(exist_ok=True)
(ROOT / "hand-built").mkdir(exist_ok=True)
subprocess.run(
    [
        "javac",
        "--release",
        "8",
        "-g:none",
        "-d",
        str(ROOT / "javac"),
        str(ROOT / "StaticInt.java"),
        str(ROOT / "LambdaFixtures.java"),
    ],
    check=True,
)


def u1(value):
    return struct.pack(">B", value)


def u2(value):
    return struct.pack(">H", value)


def utf8(value):
    data = value.encode("utf-8")
    return u1(1) + u2(len(data)) + data


# Java 1.1 class Minimal { public static int value() { return 42; } }
pool = [
    utf8("Minimal"), u1(7) + u2(1), utf8("java/lang/Object"), u1(7) + u2(3),
    utf8("value"), utf8("()I"), utf8("Code"),
]
code = bytes([0x10, 42, 0xAC])
code_attribute = u2(7) + struct.pack(">I", 12 + len(code)) + u2(1) + u2(0) + struct.pack(">I", len(code)) + code + u2(0) + u2(0)
method = u2(0x0009) + u2(5) + u2(6) + u2(1) + code_attribute
classfile = (
    bytes.fromhex("CAFEBABE") + u2(3) + u2(45) + u2(len(pool) + 1) + b"".join(pool)
    + u2(0x0021) + u2(2) + u2(4) + u2(0) + u2(0) + u2(1) + method + u2(0)
)
(ROOT / "hand-built" / "Minimal.class").write_bytes(classfile)
