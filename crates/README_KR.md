# Rust 워크스페이스

[English](README.md) | [한국어](README_KR.md) | [프로젝트 README](../README_KR.md)

Rust는 사용자에게 보이는 앱 기능을 담당합니다. GUI·CLI, 파일 탐색, 해싱, 스케줄링,
캐싱, 정규화, 분석, 검색, 보고서와 케이스 관리가 해당합니다.

## 현재 크레이트

| 크레이트 | 구현된 기반 |
|---|---|
| `metaxtract-schema` | 증거 식별, 원본 관측값, 정규화된 해석값, 출처, 진단과 레코드 |
| `metaxtract-native` | `cxx` 연결부, 제한된 입력, 콘텐츠 판별, 원본 관측값 전달, 파서 기능 조회와 오류 변환 |
| `metaxtract-core` | 스키마 노출, 엔진 정보와 확장자·MIME·컨테이너 충돌 분석 |

아직 Rust GUI·CLI·스캐너·케이스 관리 흐름을 제공하지 않습니다.
이 앱 기능은 스키마와 네이티브 연결부 위에 구현할 예정입니다.
저장소 루트에서 `cargo build --workspace --locked`로 빌드합니다.

[아키텍처](../docs/architecture_KR.md), [언어 경계](../docs/ffi_KR.md),
[전환 검증](../conformance/README_KR.md)도 참고합니다.
