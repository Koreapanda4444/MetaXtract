# 언어 전환 검증 자료

[English](README.md) | [한국어](README_KR.md) | [프로젝트 README](../README_KR.md)

이 디렉터리는 범용 루트 테스트 폴더 대신 언어 전환에 꼭 필요한 검증 자료를 모읍니다.
입력 바이트와 비교할 결과는 재현 가능하게 고정합니다. 사용자의 비공개 증거 파일이나
사용자에게서 복사한 파일은 포함하지 않습니다.

검증에는 Rust 1.88, CMake 3.24 이상, C++20 컴파일러와 Python 기준판이 필요합니다.
Windows의 Visual Studio 2026은 CMake 4.2 이상이 필요하며 CI는 4.2.3을 사용합니다.
저장소 루트에서 실행합니다.

```bash
python conformance/run.py
python conformance/run.py --release
python conformance/validate.py --cargo
cmake --preset dev
cmake --build --preset dev --config Debug
ctest --preset dev -C Debug
```

비교 전에 파일 시스템 수정 시각처럼 실행 환경에 따라 바뀌는 필드는 제외합니다.
이 자료는 기존 동작의 호환성을 확인합니다. 네이티브 엔진이 새로 추출하는 더 깊은
메타데이터는 파일 형식 명세나 여러 파서의 교차 검증으로 확인하며,
Python이 기존에 추출한 얕은 결과로 범위를 제한하지 않습니다.

실행기는 Python 기준 레코드와 경로 동작을 비교한 뒤 샘플 바이트를 Rust, C++,
Rust가 소유하는 메모리·파일 읽기 콜백으로 전달합니다. 바이트 영역, 파일 끝의 짧은 읽기,
오프셋 오버플로, 읽기 상한, 잘린 파일, 오류 변환, 스키마 규칙과 모든 파서 자원 제한을
검사합니다. 샘플 파서는 Cargo의 명시적인 `conformance` 기능에서만 활성화되며
일반 제품 빌드에는 포함되지 않습니다.

PNG·PDF 등 실제 메타데이터 레코드의 전체 동등성은 형식 어댑터를 구현한 뒤 검증합니다.
실행 결과도 이 상태를 명시하며 Python만으로 만든 레코드를 네이티브 추출 지원 범위로
계산하지 않습니다. 네이티브 빌드 도구 없이 동결된 Python 기준판만 검사하려면
`--python-only`를 사용합니다.

CI는 Linux·Windows·macOS에서 빌드와 디버그·릴리스 호환성 검증을 수행합니다.
Linux에서는 네이티브 코어와 생성된 언어 연결부에 AddressSanitizer와
UndefinedBehaviorSanitizer도 적용합니다.
Rust 정책의 회귀 검증도 `cargo test --workspace --locked`로 실행합니다.

검증이 보호하는 규칙은 [아키텍처](../docs/architecture_KR.md),
[언어 경계](../docs/ffi_KR.md), [파서 자원 제한](../docs/resource-limits_KR.md)을 참고합니다.
