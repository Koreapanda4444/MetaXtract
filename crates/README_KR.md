# Rust 워크스페이스

[English](README.md) | [한국어](README_KR.md) | [프로젝트 README](../README_KR.md)

Rust는 사용자에게 보이는 앱 기능을 담당합니다. GUI·CLI, 파일 탐색, 해싱, 스케줄링,
캐싱, 정규화, 분석, 검색, 보고서와 케이스 관리가 해당합니다.

## 현재 크레이트

| 크레이트 | 구현된 기반 |
|---|---|
| `metaxtract-schema` | 증거 식별, 원본 관측값, 정규화된 해석값, 출처, 진단과 레코드 |
| `metaxtract-native` | `cxx` 연결부, 제한된 입력, 콘텐츠 판별, 원본 관측값 전달, 파서 기능 조회와 오류 변환 |
| `metaxtract-core` | 추출 조정, 타입이 유지되는 기록, 탐색·해싱·제한된 스케줄링·캐시·지속 가능한 스캔 저장과 복구 |
| `metaxtract-worker` | 읽기 전용 사본을 입력받고 검증 가능한 프레임으로 결과를 반환하는 전용 파서 프로세스 |
| `metaxtract-analysis` | 정규화·출처·검증·발견 사항·중복·위치·비교·검색·비식별화·내보내기·보고서 |
| `metaxtract-storage` | 단일 파일 케이스, 타입 보존 저장, 페이지 조회, 이력, 가져오기, 감사, 서명 ID와 이동식 스냅샷 |
| `metaxtract-app` | 공통 앱 작업과 제한된 미리보기·스캔 처리 |
| `metaxtract-cli` | 구조화 JSON 명령·진행률·고정 종료 코드 |
| `metaxtract-gui` | 네이티브 데스크톱·페이지 검사·분석·내보내기 |

스캔·케이스 저장·분석은 Rust GUI·CLI가 공통으로 사용합니다.
전용 파서 프로세스는 코어가 실행하는 내부 실행 파일이며 사용자용 CLI가 아닙니다.
저장소 루트에서 `cargo build --workspace --locked`로 빌드합니다.

[케이스 저장](../docs/cases_KR.md), [아키텍처](../docs/architecture_KR.md), [언어 경계](../docs/ffi_KR.md),
[전환 검증](../conformance/README_KR.md)도 참고합니다.

GUI에는 스캔·페이지 검사·검색·비교·분석 화면과 내보내기·보고서·서명·번들 도구가 연결돼 있습니다. [앱 안내](../docs/application_KR.md)를 참고하세요.
