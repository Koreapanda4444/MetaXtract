# 네이티브 추출 코어

[English](README.md) | [한국어](README_KR.md) | [프로젝트 README](../README_KR.md)

C++는 바이너리 구조 파싱, 디코딩, 원본 메타데이터 추출과 허용된 네이티브 라이브러리
어댑터만 담당합니다. 케이스 상태, 정규화 정책, 분석, 검색, 보고서, GUI·CLI 동작은
C++의 역할이 아닙니다.

추출 코어는 입력·자원 제한·파서 등록·언어 연결부, 이미지·RAW·오디오·영상의 구조와
심층 메타데이터, 프로세스 내부 미디어 디코딩을 구현했습니다. LibRaw·TagLib·FFmpeg는
읽기 전용 Rust 입력을 사용합니다. 정확한 지원 범위와 한계는 추출 안내에 있습니다.
샘플 파서는 검증용 빌드에만 존재합니다.
공개 인터페이스는 내부 파서 구현보다 작게 유지해야 합니다.

저장소 루트에서 실행합니다.

```bash
cmake --preset dev
cmake --build --preset dev --config Debug
ctest --preset dev -C Debug
```

Cargo도 `metaxtract-native`를 컴파일할 때 동일한 코어를 빌드합니다.
필요한 환경과 검증 방법은 [의존성 정책](../docs/dependencies_KR.md),
[자원 제한](../docs/resource-limits_KR.md), [전환 검증](../conformance/README_KR.md)을 참고합니다.
