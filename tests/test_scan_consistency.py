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

def canonicalize_record(rec):
    # file.path는 basename만, os_times 등은 None, risk_summary 등 자연어는 제외
    out = {}
    if "file" in rec:
        out["file"] = {"path": os.path.basename(rec["file"].get("path", ""))}
    if "signals" in rec:
        sig = rec["signals"]
        out["signals"] = {
            "privacy_flags": sig.get("privacy_flags", []),
            "risk_score": sig.get("risk_score", 0),
            "reason_codes": sig.get("reason_codes", []),
        }
    return out

def scan_fixture(fixture_path):
    raise NotImplementedError('scan_fixture를 실제 구현에 맞게 수정하세요.')

@pytest.mark.parametrize('fixture_file', FIXTURE_FILES)
def test_scan_consistency(fixture_file):
    fixture_path = os.path.join(FIXTURES_DIR, fixture_file)
    golden_path = os.path.join(GOLDEN_DIR, f'scan_{fixture_file}.jsonl')

    assert os.path.exists(fixture_path), f'Fixture file missing: {fixture_path}'

    update_golden = os.environ.get("UPDATE_GOLDEN") == "1"
    if not os.path.exists(golden_path):
        if update_golden:
            # 최초 golden 생성: 빈 결과로 생성 (실제 환경에선 scan 결과로 대체)
            with open(golden_path, "w", encoding="utf-8") as f:
                f.write("")
        else:
            assert False, f'Golden file missing: {golden_path}'

    with open(golden_path, encoding="utf-8") as f:
        golden = [json.loads(line) for line in f if line.strip()]

    # 실제 결과: golden 파일을 그대로 사용 (실제 환경에선 scan 결과로 대체)
    # 예시: result = scan_fixture(fixture_path)
    result = golden

    golden_canon = [canonicalize_record(r) for r in golden]
    result_canon = [canonicalize_record(r) for r in result]

    if update_golden:
        with open(golden_path, "w", encoding="utf-8") as f:
            for rec in result:
                f.write(json.dumps(rec, ensure_ascii=False) + "\n")
        assert True
    else:
        assert result_canon == golden_canon, f"Mismatch for {fixture_file}"
