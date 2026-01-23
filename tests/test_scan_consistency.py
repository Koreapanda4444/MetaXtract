import os
import json
import pytest

FIXTURES_DIR = os.path.join(os.path.dirname(__file__), 'fixtures')
GOLDEN_DIR = os.path.join(os.path.dirname(__file__), 'golden')

FIXTURE_FILES = [
    'image_gps.jpg',
    'image_noexif.jpg',
    'sample.pdf',
    'sample.docx',
    'sample.mp4',
]

def scan_fixture(fixture_path):
    raise NotImplementedError('scan_fixture를 실제 구현에 맞게 수정하세요.')

@pytest.mark.parametrize('fixture_file', FIXTURE_FILES)
def test_scan_consistency(fixture_file):
    fixture_path = os.path.join(FIXTURES_DIR, fixture_file)
    golden_path = os.path.join(GOLDEN_DIR, f'scan_{fixture_file}.jsonl')

    assert os.path.exists(golden_path), f'Golden file missing: {golden_path}'
    assert os.path.exists(fixture_path), f'Fixture file missing: {fixture_path}'
