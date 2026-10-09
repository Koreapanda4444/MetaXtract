import pytest

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
    # set
    cache.set(str(test_file), result)
    # get
    hit = cache.get(str(test_file))
    assert hit == result
    # miss (다른 파일)
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
