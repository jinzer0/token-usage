# token-usage

로컬에 저장된 Codex와 GJC 세션 로그를 스캔해서 세션별 토큰 사용량을 집계하는 CLI 도구입니다.

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
  - JSON 출력
  - TUI 보기

## 설치 / 실행

개발 환경에서 바로 실행:

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
token-usage --json
```

테스트 fixture 같은 별도 경로를 스캔:

```bash
token-usage --codex-home tests/fixtures/codex --gjc-home tests/fixtures/gjc
```

TUI 실행:

```bash
token-usage --tui
```

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

`--verbose`를 붙이면 cache read/write, uncached input, record 수가 추가로 표시됩니다.

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
- `diagnostics`: 파싱 중 발생한 경고/오류
- `source_counts`: 스캔 파일 수, 읽은 줄 수, emit/skip record 수, missing/empty root 등

`sessions`는 총 토큰 수 내림차순으로 정렬됩니다.

## 개발

테스트 실행:

```bash
cargo test
```

현재 통합 테스트는 다음 동작을 검증합니다.

- Codex fixture 파싱과 index 기반 세션 이름 반영
- GJC duplicate message 제거와 parent session 유지
- Codex/GJC 같은 session id 충돌 방지
- `--json`과 `--tui` 동시 사용 거부

## 제한 사항

- 로컬 JSONL 로그만 읽습니다. 원격 API나 계정 사용량 페이지는 조회하지 않습니다.
- 로그 포맷이 바뀐 경우 일부 레코드는 diagnostic warning과 함께 건너뜁니다.
- Codex cumulative counter는 증가분으로 계산하며, stale/reset으로 보이는 값은 중복 방지를 위해 skip합니다.
