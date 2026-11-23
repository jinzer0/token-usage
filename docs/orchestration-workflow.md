# Orca 병렬 오케스트레이션 워크플로 가이드

이 문서는 `token-usage` 저장소에서 여러 독립 작업을 **Orca 병렬 Worktree Worker**로 분할 구현하고, 통합 및 독립 검수까지 수행하는 재사용 가능한 표준 워크플로입니다.

---

## 1. 역할 및 모델 구성

| 역할 | 실행 환경 | Agent / Model / Effort | Worktree | 책임 |
| --- | --- | --- | --- | --- |
| **Coordinator** | 현재 Antigravity CLI 세션 | Gemini 4 Neon · Medium | `main` (현재 워크스페이스) | 조사·계획 수립, 파일 소유권 분리, Worker 병렬 실행, 브랜치 머지·크로스-모듈 배선, 리뷰 이슈 최종 반영 |
| **구현 Worker** (최대 3개 동시) | Orca Supervised Worker | `antigravity` · `gemini-4-hydrogen-high` · `high` | `--worktree new-child --name <track>` | 배정된 파일 소유권 범위 내 구현, 단위/통합 테스트 작성, `fmt`/`clippy`/`test` 검증 후 브랜치 커밋 |
| **검수 Worker** (1개) | Orca Supervised Worker | `antigravity` · `gemini-4-neon-medium` · `medium` | `--worktree current` | 통합된 전체 diff 감사, 요구사항·엣지케이스·회귀 검증, `fmt`/`clippy`/`test` 독립 실행 (코드 수정 없이 리포트) |

> 모델 ID 확인: `agy models`
> Orca CLI 경로 (macOS): `/Applications/Orca.app/Contents/Resources/bin/orca` (또는 `$ORCA_CLI_COMMAND`)

---

## 2. 전체 진행 순서 (4단계)

### Phase 1. 조사 · 계획 및 트랙 분할 (Coordinator)
1. 현재 `git status`가 깨끗한지 확인하고 기준 커밋 해시(`<BASE_SHA>`)를 기록합니다.
2. 기존 테스트(`cargo test --quiet`)가 통과하는지 확인합니다.
3. 요구사항을 **최대 3개의 독립 트랙**으로 분할합니다.
   - **핵심 원칙**: 트랙 간 **파일 소유권이 겹치지 않도록** 분리합니다.
   - 각 Worker가 자신의 Worktree에서 단독으로 `cargo fmt`, `cargo clippy -D warnings`, `cargo test`를 통과할 수 있어야 합니다.
   - 크로스-트랙 인터페이스가 필요한 경우 spec에 함수 시그니처/계약만 합의해 두고, 실제 배선은 Phase 3(통합)에서 Coordinator가 수행합니다.
4. 각 트랙의 작업 지시서를 별도 Markdown 파일(`--spec-file`)로 작성합니다.
   - 필수 포함 항목: 담당 파일 목록(수정 금지 파일 명시), 상세 요구사항, 검증 명령, 커밋 지시, 마지막 줄 `Placement: this machine (local), the user named it.`

### Phase 2. 병렬 구현 (구현 Workers — Gemini 4 Hydrogen · High)
- 한 번의 셸 명령에서 백그라운드(`&`)로 최대 3개 Worker를 동시에 시작하고 `wait`로 receipt를 수집합니다.
- 모든 `worker-start`에는 `--environment local`을 명시합니다.

```sh
d=$(mktemp -d)
ORCA=/Applications/Orca.app/Contents/Resources/bin/orca
SDIR=/path/to/specs

"$ORCA" orchestration worker-start \
  --environment local \
  --spec-file "$SDIR/w1-spec.md" \
  --task-title "W1: <short title>" \
  --worktree new-child --name w1-track-name \
  --agent antigravity --model gemini-4-hydrogen-high --effort high \
  >"$d/w1.out" 2>"$d/w1.err" & p1=$!

"$ORCA" orchestration worker-start \
  --environment local \
  --spec-file "$SDIR/w2-spec.md" \
  --task-title "W2: <short title>" \
  --worktree new-child --name w2-track-name \
  --agent antigravity --model gemini-4-hydrogen-high --effort high \
  >"$d/w2.out" 2>"$d/w2.err" & p2=$!

# 필요 시 최대 w3까지 동일 패턴 추가
s=0; wait "$p1" || s=$?; echo "w1 exit $s"
s=0; wait "$p2" || s=$?; echo "w2 exit $s"
for f in "$d"/*; do echo "== $f"; cat "$f"; done
```

- Worker 시작이 완료되면 Coordinator는 **턴을 종료**하고 Orca의 `worker_done` 알림을 기다립니다 (폴링/watch 스크립트 금지).

### Phase 3. 통합 · 크로스-모듈 배선 (Coordinator)
1. 모든 구현 Worker가 `outcome: "succeeded"`로 보고하면, 각 Worktree 브랜치(`orca/<name>`)를 `main`에 순차 머지합니다:
   ```sh
   git merge --no-edit orca/w1-track-name
   git merge --no-edit orca/w2-track-name
   ```
2. 트랙 간 배선(새 도메인 필드 ↔ TUI 위젯, 새 테마/포맷 유틸 ↔ 위젯, CLI ↔ TUI 진입점 등)과 중복 헬퍼 정리를 수행합니다.
3. 전체 검증을 실행하고 통합 커밋을 생성합니다:
   ```sh
   cargo fmt --all -- --check && \
   cargo clippy --all-targets --all-features -- -D warnings && \
   cargo test --quiet
   ```

### Phase 4. 독립 검수 · 테스트 및 최종 수정 (Review Worker — Gemini 4 Neon · Medium)
1. 검수 전용 `--spec-file`에 기준 커밋(`<BASE_SHA>..HEAD`), 요구사항 체크리스트, 읽기 전용 감사 원칙, 검증 명령을 명시합니다.
2. 검수 Worker를 `--worktree current`로 실행합니다:
   ```sh
   /Applications/Orca.app/Contents/Resources/bin/orca orchestration worker-start \
     --environment local \
     --spec-file "$SDIR/review-spec.md" \
     --task-title "Review: Independent audit & tests" \
     --worktree current \
     --agent antigravity --model gemini-4-neon-medium --effort medium
   ```
3. 검수 완료(`worker_done`) 후 발견된 Critical / Medium / Minor 이슈를 수정하고 회귀 테스트를 추가합니다.
4. 최종 검증(`cargo fmt --all -- --check`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test`) 통과 후 커밋하고 사용자에게 보고합니다.

---

## 3. 운영 규칙 요약
- **Heredoc(`<<`) / Here-string(`<<<`) 금지**: 멀티라인 텍스트는 항상 파일로 작성해 `--spec-file`로 전달합니다.
- **폴링 금지**: `worker-start` 이후에는 턴을 마치고 Orca의 자동 깨우기(wakeup)를 기다립니다.
- **작업 후 정리**: 머지와 검수가 끝난 Worktree는 필요 시 `orca worktree` / `orca orchestration worker-release`로 정리할 수 있습니다.
