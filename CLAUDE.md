# duru — AI Working Notes

> Workflow guardrails, quick references, and traps for AI coding assistants.
> User-facing docs are in [`README.md`](README.md). Architecture & patterns are in [`docs/ENGINEERING_HANDBOOK.md`](docs/ENGINEERING_HANDBOOK.md).

duru는 Claude Code의 `~/.claude/`를 탐색하는 Rust TUI다. ratatui 기반, single-threaded event loop, no async runtime, no global state. 안정성과 일관된 시각 언어가 가장 중요한 두 가치.

---

## Engineering Handbook

코딩 패턴과 아키텍처 가이드는 [`docs/ENGINEERING_HANDBOOK.md`](docs/ENGINEERING_HANDBOOK.md)에 있다.

**양방향 링크 시스템:**
- 코드의 `//! @handbook X.Y-slug` → handbook 섹션 참조
- 문서의 `<!-- @code path -->` → 소스 파일 참조
- 변경 시 **양쪽 동기화 필수** — handbook의 섹션을 변경하면 그 섹션을 가리키는 모든 `@handbook` 마커도 갱신
- 마커 검색: `grep -rn "@handbook" src/ tests/`
- `/update-handbook` 스킬이 drift를 자동 감지

---

## Quick Reference

### Handbook 섹션 빠른 참조

| 찾는 것 | HANDBOOK 섹션 |
|---------|--------------|
| 모듈 의존성 / 메인 루프 | 2 |
| Type design (enums, domain structs) | 3 |
| Filesystem scan, project name decoding | 4.1-4.2 |
| JSONL transcript 파싱, cache TTL 추론 | 4.3-4.5 |
| Session cache 갱신, /clear 감지 | 4.6-4.7 |
| Registry schema, PID liveness | 4.8-4.9 |
| Ratatui composition, focus, sort indicators | 5 |
| Theme + 색 사용 컨벤션 | 6 |
| Markdown 렌더 (frontmatter, style stack) | 7 |
| Hook script invariants, jq merge, backup | 8 |
| Atomic write, no-panic, custom-home | 9 |
| Test conventions (tempdir, jq guard) | 10 |

### Boilerplate Reference

새 코드를 추가할 때 비슷한 기존 파일을 참조한다:

| 패턴 | 참고 파일 |
|------|----------|
| Pure-state struct + key handler | `src/app.rs` |
| Filesystem read + decode + sort | `src/scan.rs` |
| JSONL streaming parse + cache | `src/sessions.rs` |
| Schema-versioned JSON serde + cleanup | `src/registry.rs` |
| Ratatui layout + Block composition | `src/ui.rs` |
| Style enum (no raw RGB) | `src/theme.rs` |
| pulldown-cmark Event handler | `src/markdown.rs` |
| jq pipeline + atomic rename | `src/hooks_install.rs::merge_settings` |
| Embedded shell via include_str! | `src/hook_scripts.rs` |
| Hook bash script (always exit 0) | `src/_hook_scripts/session-start.sh` |
| Integration test (tempdir + jq guard) | `tests/hook_scripts.rs` |

### Top Traps

1. **두 번째 `BufReader`를 같은 `File`에 만들지 말 것** — OS file offset 공유 때문에 두 번째는 빈 결과를 받는다. `parse_tail()`이 single BufReader + `lines.next()`로 partial 첫 줄을 버리는 패턴 사용. (handbook §4.3)

2. **State enum에 `_ =>` 절대 금지** — 새 variant 추가 시 컴파일러가 매칭 누락을 잡아주도록 exhaustive match 유지. (handbook §3.1)

3. **`std::env::set_var` 사용 금지** — Rust 2024에서 unsafe. 결정 로직은 pure function으로 추출(`resolve_editor_from`처럼). (handbook §9.4)

4. **Hook script는 항상 `exit 0`** — Claude Code를 차단하면 안 된다. malformed input, jq 실패, 권한 에러 모두 swallow. (handbook §8.2)

5. **settings.json 직접 쓰지 말 것** — 항상 jq pipeline + tmp file + serde_json 검증 + atomic rename. 부분 파일이 디스크에 보이면 사용자의 다른 hook이 깨진다. (handbook §9.2)

6. **Ratatui Color::Rgb 직접 사용 금지** — 11-color Theme 팔레트만 사용. 새 색이 필요하면 dark/light 둘 다 정의. (handbook §6.2)

7. **`unwrap()`/`expect()`을 UI 루프 안에 넣지 말 것** — panic이 raw mode를 복구 못 하고 터미널을 corrupt한다. fallback path로 처리. (handbook §9.1)

8. **Registry schema 변경 시 `CURRENT_SCHEMA_VERSION` bump** — 또는 새 필드를 `#[serde(default)]` 옵셔널로 추가. forward-compatibility 유지. (handbook §4.8)

9. **TUI 동작 변경은 사람이 확인** — type check / cargo test가 비주얼 회귀를 잡지 못한다. 변경 후 `cargo run -- --demo`로 직접 확인하거나 사용자에게 수동 검증 부탁.

10. **PR title은 Conventional Commits** — release-drafter가 type 기반으로 분류. `feat:`, `fix:`, `docs:`, `refactor:`, `perf:`, `test:`, `chore:`만 허용.

---

## Build & Test

```bash
cargo build              # production build (release: cargo build --release)
cargo test               # unit + integration tests
cargo clippy -- -D warnings  # lint
cargo run -- --demo      # 실제 ~/.claude/ 없이 데모 데이터로 TUI 확인
cargo run -- --theme light   # light 테마 강제
```

`tests/hook_scripts.rs`는 `jq`가 PATH에 있어야 통과(없으면 skip). macOS: `brew install jq`.

---

## Workflow Rules

### Git
- **Commit은 반드시 `/group-commit` 스킬 호출** — 시스템 내장 커밋 절차 사용 금지.
- `Co-Authored-By` 줄 / `Generated with Claude Code` 등 메타정보 포함 금지.
- 커밋 전 `cargo build && cargo test` 통과 확인.

### Branch Targeting
- `main`: 버전 범프 / CI 파일만 직접 커밋. 그 외 코드 변경은 `git checkout -b feature/{type}-{description}` 필수.
- `feature/*`: 그대로 진행.

### Sync Tags
- `test-sync`, `handbook-sync`는 **로컬 전용 태그** (push 금지).
- 태그 이동 조건:
  1. `/update-test-map` 실행 완료 — 식별된 모든 테스트/마커 적용 후
  2. `/update-handbook` 실행 완료 — handbook + 이 CLAUDE.md 의 Quick Reference / Top Traps 갱신 적용 후
- 두 skill을 항상 pair로 실행 (cross-dependency drift 방지).

### Language
- GitHub-facing 텍스트(릴리스 노트, PR description, commit title): 영어
- Commit message 본문: 한글 허용
- 사용자 대화: 사용자 언어
