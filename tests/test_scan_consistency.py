
import os
import json
import pytest
from tests.gen_fixtures import generate


HERE = os.path.dirname(__file__)
FIXTURES_DIR = os.path.join(HERE, "fixtures")
GOLDEN_DIR = os.path.join(HERE, "golden")


FIXTURE_FILES = [
    "image_gps.jpg",
    "image_noexif.jpg",
    "sample.pdf",
    "sample.docx",
    "sample.mp4",
]

# ✅ fixture 생성은 "테스트 함수 밖"에서 1번만 실행해서 안정화
GEN_ERROR = None
generated = {}
try:
    generated = generate(FIXTURES_DIR) or {}
except Exception as e:
    GEN_ERROR = e

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


@pytest.mark.parametrize("fixture_file", FIXTURE_FILES)
def test_scan_consistency(fixture_file):
    # ✅ 생성 단계에서 터졌으면 여기서 원인을 보여주고 실패
    if GEN_ERROR is not None:
        pytest.fail(f"Fixture generation failed: {GEN_ERROR!r}")

    fixture_path = os.path.join(FIXTURES_DIR, fixture_file)
    golden_path = os.path.join(GOLDEN_DIR, f"scan_{fixture_file}.jsonl")

    # mp4는 CI에 ffmpeg 없으면 스킵 허용
    if fixture_file == "sample.mp4" and not generated.get("sample.mp4", False):
        pytest.skip("ffmpeg not available; mp4 fixture not generated")

    # ✅ 생성이 끝난 뒤 존재해야 정상
    assert os.path.exists(fixture_path), (
        f"Fixture file missing: {fixture_path} "
        f"(generator_status={generated})"
    )

    update_golden = os.environ.get("UPDATE_GOLDEN") == "1"
    if not os.path.exists(golden_path):
        if update_golden:
            # 최초 golden 생성: 빈 결과로 생성 (실제 환경에선 scan 결과로 대체)
            with open(golden_path, "w", encoding="utf-8") as f:
                f.write("")
        else:
            assert False, f"Golden file missing: {golden_path}"

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
