# token-usage

로컬에 저장된 Codex와 GJC 세션 로그를 스캔해서 세션별/기간별 토큰 사용량을 집계하는 CLI 도구입니다.

## Quick Install

Rust toolchain 없이 최신 GitHub Release 바이너리를 설치합니다.

```bash
curl -fsSL https://raw.githubusercontent.com/jinzer0/token-usage/main/install.sh | sh
```

기본 설치 경로는 `$HOME/.local/bin`입니다. 다른 경로를 쓰려면:

```bash
TOKEN_USAGE_INSTALL_DIR=/usr/local/bin \
  sh -c "$(curl -fsSL https://raw.githubusercontent.com/jinzer0/token-usage/main/install.sh)"
```

설치 후 확인:

```bash
token-usage --version
```

## Manual Install

1. https://github.com/jinzer0/token-usage/releases 에서 자신의 OS/architecture에 맞는 archive를 다운로드합니다.
2. 압축을 풉니다.
3. `token-usage` 바이너리를 `$PATH`에 포함된 디렉터리로 복사합니다.

예:

```bash
curl -LO https://github.com/jinzer0/token-usage/releases/download/v0.2.0/token-usage-v0.2.0-aarch64-apple-darwin.tar.gz
tar -xzf token-usage-v0.2.0-aarch64-apple-darwin.tar.gz
mkdir -p ~/.local/bin
mv token-usage ~/.local/bin/
token-usage --version
```

Release에는 `SHA256SUMS`가 포함되며, `install.sh`는 가능한 환경에서 checksum을 검증합니다.

## 지원 범위

- Codex 로그: 기본 경로 `~/.codex`
  - `sessions/**/*.jsonl` 스캔
  - `session_index.jsonl`의 세션 이름 반영
- GJC 로그: 기본 경로 `~/.gjc/agent/sessions`
  - `**/*.jsonl` 스캔
  - 중복 assistant usage 메시지 제거
  - 부모 세션 정보와 branch별 reasoning effort 반영
- 출력 형식
  - 기본 텍스트 요약
  - 세션 상세 보기
  - 기간별 집계 테이블
  - JSON 출력
  - TUI 보기

## 개발 환경 실행

```bash
cargo run -- [OPTIONS] [SESSION]
```

릴리스 바이너리 빌드:

```bash
cargo build --release
./target/release/token-usage [OPTIONS] [SESSION]
```

## 사용법

```text
token-usage [OPTIONS] [SESSION]
```

### 주요 옵션

| 옵션 | 설명 |
| --- | --- |
| `--client <all|codex|gjc>` | 스캔할 클라이언트 선택. 기본값은 `all` |
| `--today` | 로컬 timezone 기준 오늘 record만 포함 |
| `--since <7d|30d|YYYY-MM-DD>` | 지정 시점 이후 record만 포함 |
| `--until <YYYY-MM-DD>` | 지정 날짜까지의 record만 포함. 해당 날짜 전체를 포함 |
| `--group-by <day|week|month>` | 기간별 bucket 테이블 출력 |
| `--json` | 집계 결과를 pretty JSON으로 출력 |
| `--verbose` | 캐시 토큰, record 수 등 추가 정보 출력 |
| `--tui` | 터미널 UI로 결과 표시 |
| `--codex-home <PATH>` | Codex 홈 경로 override |
| `--gjc-home <PATH>` | GJC 세션 로그 루트 override |
| `[SESSION]` | 특정 세션 상세 출력. 세션 id, prefix, 이름 prefix, `client:id` 사용 가능 |

`--json`과 `--tui`는 동시에 사용할 수 없습니다.

## 예시

전체 요약:

```bash
token-usage
```

오늘 사용량:

```bash
token-usage --today
```

최근 7일 사용량:

```bash
token-usage --since 7d
```

최근 30일을 일별로 집계:

```bash
token-usage --since 30d --group-by day
```

특정 날짜 범위:

```bash
token-usage --since 2026-09-01 --until 2026-09-10
```

월별 집계:

```bash
token-usage --group-by month
```

GJC 로그만 요약:

```bash
token-usage --client gjc
```

상세 토큰 정보를 포함한 요약:

```bash
token-usage --verbose
```

특정 세션 상세 보기:

```bash
token-usage gjc:01K4EXAMPLESESSIONID
```

JSON으로 내보내기:

```bash
token-usage --json --since 7d
```

테스트 fixture 같은 별도 경로를 스캔:

```bash
token-usage --codex-home tests/fixtures/codex --gjc-home tests/fixtures/gjc
```

TUI 실행:

```bash
token-usage --tui
```

## 기간 필터 정책

- 기간 필터가 없으면 timestamp가 없는 record도 기존처럼 포함합니다.
- `--today`, `--since`, `--until` 중 하나라도 활성화되면 timestamp가 없는 record는 기간 집계에서 제외됩니다.
- 제외된 timestamp 없는 record 수는 `source_counts.records_missing_timestamp_filtered`에 기록됩니다.
- 범위 밖 record 수는 `source_counts.records_time_filtered`에 기록됩니다.
- `--today`와 날짜 기반 필터는 사용자의 local timezone 기준입니다.

## 기본 텍스트 출력

요약 출력은 세션별로 다음 값을 보여줍니다.

```text
client session total input output reasoning
```

- `client`: `codex` 또는 `gjc`
- `session`: 세션 이름이 있으면 이름, 없으면 세션 id
- `total`: 총 토큰 수
- `input`: 입력 토큰 수
- `output`: 출력 토큰 수
- `reasoning`: 확인된 reasoning 토큰 수
  - `+`가 붙으면 일부 레코드에서 reasoning 토큰이 명시되지 않았다는 뜻입니다.

`--group-by`를 사용하면 기간별 테이블을 출력합니다.

```text
date         total       input       output      reasoning   records
2026-09-08    1,210,430     850,220     210,310     149,900       12
```

## 세션 상세 출력

`SESSION` 인자를 넘기면 해당 세션의 모델별, reasoning effort별 사용량을 보여줍니다.

세션 선택자는 다음 방식으로 해석됩니다.

1. `client:id`와 정확히 일치
2. 세션 id 전체 또는 prefix
3. 세션 이름 전체 또는 prefix

여러 세션이 동시에 매칭되면 ambiguous 에러와 후보 목록을 출력합니다.

## JSON 구조

`--json` 출력의 최상위 구조는 다음 필드를 포함합니다.

- `generated_at`: 집계 생성 시각
- `sessions`: 세션별 집계 목록
- `timeline`: `--group-by` 사용 시 기간 bucket 목록
- `periods`: TUI/요약용 today, 7 days, 30 days token totals
- `diagnostics`: 파싱 중 발생한 경고/오류
- `source_counts`: 스캔 파일 수, 읽은 줄 수, emit/skip/filter record 수, missing/empty root 등

`sessions`는 총 토큰 수 내림차순으로 정렬됩니다.

## TUI

```bash
token-usage --tui
```

상단 헤더에는 전체 세션 수와 토큰 수, 오늘/7일/30일 사용량 요약, 새로고침 시간이 표시됩니다.

주요 키:

```text
j/k 또는 ↑/↓   세션 이동
J/K            상세 패널 스크롤
/              세션 검색
v              상세 breakdown 토글
r              새로고침
?              도움말
q              종료
```

## 개발

검증 명령:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

현재 통합 테스트는 다음 동작을 검증합니다.

- Codex fixture 파싱과 index 기반 세션 이름 반영
- GJC duplicate message 제거와 parent session 유지
- malformed JSONL, 누락 필드, invalid timestamp, unknown reasoning effort, empty/missing directory
- Codex cumulative stale/reset 처리
- 기간 필터와 day/week/month grouping
- `--json`과 `--tui` 동시 사용 runtime validation
- 잘못된 `--since`, `--group-by`, inverted range 처리

## Release

- `.github/workflows/ci.yml`: PR/main push에서 fmt, clippy, test 실행
- `.github/workflows/release.yml`: `v*` tag push 시 release archive와 `SHA256SUMS` 생성

지원 archive:

```text
token-usage-vX.Y.Z-x86_64-unknown-linux-gnu.tar.gz
token-usage-vX.Y.Z-aarch64-apple-darwin.tar.gz
token-usage-vX.Y.Z-x86_64-apple-darwin.tar.gz
token-usage-vX.Y.Z-x86_64-pc-windows-msvc.zip
```

LICENSE 파일이 없는 경우 archive에는 binary와 README만 포함됩니다.

## 제한 사항

- 로컬 JSONL 로그만 읽습니다. 원격 API나 계정 사용량 페이지는 조회하지 않습니다.
- 로그 포맷이 바뀐 경우 일부 레코드는 diagnostic warning과 함께 건너뜁니다.
- Codex cumulative counter는 증가분으로 계산하며, stale/reset으로 보이는 값은 중복 방지를 위해 skip합니다.
- Linux arm64 release archive는 아직 기본 release matrix에 포함하지 않았습니다.
