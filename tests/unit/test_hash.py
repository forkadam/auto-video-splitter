import hashlib
from pathlib import Path
import pytest
from vsplit.core.hash import partial_hash, content_hash

def test_content_hash_matches_sha256(tmp_path: Path):
    f = tmp_path / "x.txt"
    f.write_bytes(b"hello world")
    expected = "sha256:" + hashlib.sha256(b"hello world").hexdigest()
    assert content_hash(f) == expected

def test_partial_hash_small_file(tmp_path: Path):
    f = tmp_path / "small.bin"
    data = b"a" * 100
    f.write_bytes(data)
    expected_body = data + len(data).to_bytes(8, "big")
    expected = "sha256:" + hashlib.sha256(expected_body).hexdigest()
    assert partial_hash(f) == expected

def test_partial_hash_large_file(tmp_path: Path):
    f = tmp_path / "big.bin"
    size = 3 * 1024 * 1024     # 3MB so first 1MB and last 1MB don't overlap
    payload = b"\x00" * size
    f.write_bytes(payload)
    first = payload[:1024 * 1024]
    last = payload[-1024 * 1024:]
    expected_body = first + last + size.to_bytes(8, "big")
    expected = "sha256:" + hashlib.sha256(expected_body).hexdigest()
    assert partial_hash(f) == expected
