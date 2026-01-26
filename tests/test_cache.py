from cache import CacheStore


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
