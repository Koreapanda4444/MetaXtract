import pytest

import metaxtract.core.cache as cache_module
import metaxtract.core.files as files_module
import metaxtract.core.scanner as scanner_module
from metaxtract.core.cache import CacheStore
from metaxtract.core.scanner import scan_file, scan_path


def _make_symlink(link, target, *, directory=False):
    try:
        link.symlink_to(target, target_is_directory=directory)
    except (NotImplementedError, OSError):
        pytest.skip("symbolic links are not available in this environment")


def test_cache_set_get(tmp_path):
    cache = CacheStore(cache_dir=tmp_path)
    test_file = tmp_path / "test.txt"
    test_file.write_text("hello world")
    result = {"foo": 123}
    cache.set(str(test_file), result)
    hit = cache.get(str(test_file))
    assert hit == result
    test_file2 = tmp_path / "test2.txt"
    test_file2.write_text("other")
    assert cache.get(str(test_file2)) is None


def test_cache_purge(tmp_path):
    cache = CacheStore(cache_dir=tmp_path)
    test_file = tmp_path / "test.txt"
    test_file.write_text("abc")
    cache.set(str(test_file), {"bar": 1})
    assert cache.get(str(test_file)) is not None
    cache.purge()
    assert cache.get(str(test_file)) is None


def test_cache_stats(tmp_path):
    cache = CacheStore(cache_dir=tmp_path)
    test_file = tmp_path / "test.txt"
    test_file.write_text("abc")
    cache.set(str(test_file), {"bar": 1})
    stats = cache.stats()
    assert stats["entries"] == 1
    assert stats["size"] > 0


def test_scan_cache_hits_do_not_mutate_frozen_records(tmp_path):
    source = tmp_path / "evidence.txt"
    source.write_text("same", encoding="utf-8")
    cache = CacheStore(cache_dir=tmp_path / "cache")

    first = scan_file(source, base=tmp_path, cache=cache)
    second = scan_file(source, base=tmp_path, cache=cache)

    assert "cache_hit" not in first.metadata
    assert second.metadata["cache_hit"] is True
    assert second.path == "evidence.txt"


def test_same_content_at_different_paths_has_separate_cache_entries(tmp_path):
    first_path = tmp_path / "first.txt"
    second_path = tmp_path / "second.txt"
    first_path.write_text("same", encoding="utf-8")
    second_path.write_text("same", encoding="utf-8")
    cache = CacheStore(cache_dir=tmp_path / "cache")

    first = scan_file(first_path, base=tmp_path, cache=cache)
    second = scan_file(second_path, base=tmp_path, cache=cache)

    assert first.path == "first.txt"
    assert second.path == "second.txt"
    assert cache.stats()["entries"] == 2


def test_corrupt_cache_is_treated_as_empty(tmp_path):
    source = tmp_path / "evidence.txt"
    source.write_text("data", encoding="utf-8")
    cache = CacheStore(cache_dir=tmp_path / "cache")
    cache.index_path.write_text("{broken", encoding="utf-8")

    assert cache.get(source) is None
    assert cache.stats()["entries"] == 0


def test_scan_rejects_symlinked_root(tmp_path):
    evidence = tmp_path / "evidence"
    evidence.mkdir()
    root_link = tmp_path / "root-link"
    _make_symlink(root_link, evidence, directory=True)

    with pytest.raises(ValueError, match="symbolic links"):
        scan_path(root_link, cache_enabled=False)


def test_scan_rejects_symlinked_files_and_directories(tmp_path):
    evidence = tmp_path / "evidence"
    evidence.mkdir()
    outside_file = tmp_path / "outside.txt"
    outside_file.write_text("outside", encoding="utf-8")
    file_link = evidence / "file-link.txt"
    _make_symlink(file_link, outside_file)

    with pytest.raises(ValueError, match="symbolic links"):
        scan_path(evidence, cache_enabled=False)

    file_link.unlink()
    outside_directory = tmp_path / "outside"
    outside_directory.mkdir()
    directory_link = evidence / "directory-link"
    _make_symlink(directory_link, outside_directory, directory=True)

    with pytest.raises(ValueError, match="symbolic links"):
        scan_path(evidence, cache_enabled=False)


def test_scan_file_rejects_paths_outside_base(tmp_path):
    evidence = tmp_path / "evidence"
    evidence.mkdir()
    outside = tmp_path / "outside.txt"
    outside.write_text("outside", encoding="utf-8")

    with pytest.raises(ValueError, match="escapes scan root"):
        scan_file(outside, base=evidence, cache_enabled=False)


def test_sha_cache_hashes_each_file_once_per_scan(tmp_path, monkeypatch):
    evidence = tmp_path / "evidence"
    evidence.mkdir()
    for index in range(3):
        (evidence / f"file-{index}.txt").write_text(str(index), encoding="utf-8")
    cache = CacheStore(tmp_path / "cache")
    real_hash = files_module.sha256_file
    hash_calls = 0

    def counting_hash(path, chunk_size=1024 * 1024):
        nonlocal hash_calls
        hash_calls += 1
        return real_hash(path, chunk_size)

    monkeypatch.setattr(scanner_module, "sha256_file", counting_hash)
    monkeypatch.setattr(cache_module, "sha256_file", counting_hash)

    first = scan_path(evidence, cache=cache)
    second = scan_path(evidence, cache=cache)

    assert hash_calls == 6
    assert all("cache_hit" not in record.metadata for record in first)
    assert all(record.metadata["cache_hit"] is True for record in second)


def test_mtime_cache_skips_hashing_on_hits(tmp_path, monkeypatch):
    evidence = tmp_path / "evidence"
    evidence.mkdir()
    (evidence / "file.txt").write_text("data", encoding="utf-8")
    cache = CacheStore(tmp_path / "cache")
    real_hash = files_module.sha256_file
    hash_calls = 0

    def counting_hash(path, chunk_size=1024 * 1024):
        nonlocal hash_calls
        hash_calls += 1
        return real_hash(path, chunk_size)

    monkeypatch.setattr(scanner_module, "sha256_file", counting_hash)

    scan_path(evidence, cache=cache, cache_mode="mtime")
    second = scan_path(evidence, cache=cache, cache_mode="mtime")

    assert hash_calls == 1
    assert second[0].metadata["cache_hit"] is True


def test_scan_batches_cache_index_writes(tmp_path, monkeypatch):
    evidence = tmp_path / "evidence"
    evidence.mkdir()
    for index in range(4):
        (evidence / f"file-{index}.txt").write_text(str(index), encoding="utf-8")
    cache_dir = tmp_path / "cache"
    cache = CacheStore(cache_dir)
    original_save = cache._save_entries
    save_calls = 0

    def counting_save():
        nonlocal save_calls
        save_calls += 1
        original_save()

    monkeypatch.setattr(cache, "_save_entries", counting_save)

    scan_path(evidence, cache=cache)

    assert save_calls == 1
    assert CacheStore(cache_dir).stats()["entries"] == 4
