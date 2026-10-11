# Python 동작 기준

[English](README.md) | [한국어](README_KR.md) | [프로젝트 README](../../README_KR.md)

Git 커밋 `374e79599fe8416652af4ee724780cf59537cc19`의 Python 구현을
Rust + C++ 전환 시 기존 동작의 기준으로 사용합니다.

새 엔진이 추가로 추출하는 메타데이터의 정답을 이 기준판이 결정하지는 않습니다.
기준판은 기존 사용자가 이용하던 동작을 보호합니다. 안전한 상대 경로, SHA-256 값,
레코드 검증, 케이스와 번들 호환성, CLI 종료 처리와 기존 데스크톱 사용 흐름이 해당합니다.

[`baseline.json`](baseline.json)은 대표 결과에서 안정적으로 비교할 항목을 기록합니다.
파일 시스템 수정 시각처럼 환경에 따라 달라지는 필드는 제외합니다.
실행 가능한 검증기와 전환용 샘플은 [`conformance/`](../../conformance/README_KR.md)에 있습니다.

동결된 소스는 [`legacy/python/`](../../legacy/python/README_KR.md)에 있습니다.
비교를 위해 `src/` 트리를 유지합니다. 문서는 기준 구현을 변경하지 않고 갱신할 수 있습니다.
대체 조건은 [아키텍처](../../docs/architecture_KR.md)를 참고합니다.
