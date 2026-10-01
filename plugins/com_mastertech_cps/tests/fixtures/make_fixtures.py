"""Regenerates the synthetic SUPERAntiSpyware SETTINGS databases used by the cps plugin tests.

Values mirror the layout of a real SAS 10 install; the registration code is fake.
Run from this directory: python make_fixtures.py
"""
import os
import sqlite3
import struct

STR, WSTR, DWORD, BINARY = 256, 257, 259, 263


def ansi(s):
    return s.encode("ascii") + b"\0"


def wide(s):
    return s.encode("utf-16-le") + b"\0\0"


def systemtime(y, mo, d, h=0, mi=0, s=0):
    return struct.pack("<8H", y, mo, 0, d, h, mi, s, 0)


def build(path, rows, filler=0):
    if os.path.exists(path):
        os.remove(path)
    db = sqlite3.connect(path)
    db.execute("PRAGMA page_size = 1024")
    db.execute("CREATE TABLE SETTINGS (id INTEGER, name TEXT COLLATE NOCASE, type INTEGER, data BLOB)")
    all_rows = list(rows) + [(f"Filler{i:03d}", STR, ansi(f"filler-value-{i:03d}")) for i in range(filler)]
    db.executemany("INSERT INTO SETTINGS (id, name, type, data) VALUES (?, ?, ?, ?)",
                   [(i + 1, n, t, d) for i, (n, t, d) in enumerate(all_rows)])
    db.commit()
    db.close()


common = [
    ("SetupWizardComplete", STR, ansi("yes")),
    ("InstallType", STR, ansi("FREE")),
    ("LastUpdateVersion", STR, ansi("10, 0, 0, 1290")),
    ("InstallationTime", BINARY, systemtime(2024, 10, 6, 14, 5, 28)),
    ("ApplicationPathW", WSTR, wide("C:\\Program Files\\SUPERAntiSpyware\\")),
    ("BigBlob", BINARY, bytes(range(256)) * 8),
]

build("sas_alluser_registered.db3", common + [
    ("Registration", DWORD, struct.pack("<i", 0)),
    ("RegCodeEx", STR, ansi("TESTREGCODE0001")),
    ("SubscriptionExpiration", BINARY, systemtime(2027, 9, 29)),
    ("ExpireToFree", STR, ansi("no")),
    ("SavedExpirationDate", BINARY, bytes(16)),
], filler=120)

build("sas_alluser_unregistered.db3", common + [
    ("Registration", DWORD, struct.pack("<i", 0)),
    ("RegCodeEx", STR, b"\0"),
    ("ExpireToFree", STR, ansi("no")),
    ("SavedExpirationDate", BINARY, bytes(16)),
])

build("sas_currentuser.db3", [
    ("PreConfigurationComplete", STR, ansi("yes")),
    ("EnableRealTimeProtection", STR, ansi("yes")),
    ("UpgradeToProfessionalCompleted", STR, ansi("yes")),
    ("ScanAutoCleanLogsDays", DWORD, struct.pack("<i", 30)),
])
