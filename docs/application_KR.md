# Rust 앱

[English](application.md) | [한국어](application_KR.md)

명령행 앱과 전용 파서를 함께 빌드합니다.

```bash
cargo build -p metaxtract-cli -p metaxtract-worker --locked
cargo run -p metaxtract-cli --locked -- --help
cargo run -p metaxtract-cli --locked -- scan /path/to/evidence --case investigation.mxcase --progress
cargo run -p metaxtract-cli --locked -- case history investigation.mxcase
cargo run -p metaxtract-cli --locked -- verify investigation.mxcase
```

Windows에서는 `.exe`를 붙입니다. Cargo가 빌드한 `metaxtract-worker`와 검토된 네이티브
라이브러리를 앱 옆에 유지합니다. `inspect FILE`은 독립된 임시 케이스를 만들고 `compare CASE
OLD_SCAN NEW_SCAN`은 저장된 스캔을 비교합니다. 케이스 명령으로 생성·정보·이력·노트를
다룹니다. 출력은 JSON이며 스캔 진행은 stderr로 보냅니다. 운영체제 고유 경로 단위를 보존합니다.

결과는 단일 파일 케이스에 점진적으로 저장하고 실행 중 체크포인트는 별도 `.scan.sqlite`
저널에 유지합니다. 일시정지 중에는 저널도 보존해야 합니다. 같은 원본·케이스와
`--resume RUN --scan CASE_SCAN_ID`로 재개하고 저널 경로를 바꿨다면 `--journal PATH`도
지정합니다. 완료 결과와 원자적 영수증을 대조해 중단 구간을 복구하며 중복 저장을 방지합니다.
저널과 케이스는 다른 파일이어야 하며 출력 파일은 탐색에서 제외합니다.
