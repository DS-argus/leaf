#!/usr/bin/env python3
"""Controlled tmux smoke harness for keyboard link navigation.

This is deliberately a smoke harness rather than a unit-test runner. It starts the requested
binary in an isolated tmux server and HOME, drives the approved keyboard contract, captures both
plain and ANSI pane output, and writes JSON scenario evidence. The harness is intentionally not
invoked by the repository test suite.

By default clipboard commands are temporary stubs. ``--live-effects`` preserves/restores the
real clipboard, starts a localhost-only HTTP server, and exercises the accepted ``o`` path with a
localhost URL. No remote URL is used by this script.
"""

from __future__ import annotations

import argparse
import datetime as _datetime
import http.server
import json
import os
import shutil
import shlex
from pathlib import Path
import re
import subprocess
import tempfile
import threading
import time
import uuid
from typing import Any, Callable, Optional


DEFAULT_WIDTH = 100
DEFAULT_HEIGHT = 24
DEFAULT_TIMEOUT = 5.0

LONG_URL = (
    "https://example.test/long?emoji=é&query="
    "abcdefghijklmnopqrstuvwxyz0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ"
    "-wrapped-link-destination"
)

FIXTURE_TEMPLATE = """# Keyboard link-navigation smoke fixture

The first visible link is [First](https://example.test/first), followed by a repeated
[First again](https://example.test/first). This is a long destination [Long URL]({long_url})
with [Anchor](#local-target), a [Relative path](docs/relative.md), and a
[Mail target](mailto:link@example.test).

## Tall heading with a late link

### {heading_words} [TargetZ](https://example.test/heading-late)

| Wrapped left column | Right column |
| :---: | ---: |
| [A A A A A A A A A A A A A A A A](https://example.test/a) [C](https://example.test/c) | [B](https://example.test/b) |
{local_link}

The final occurrence is [Last](mailto:last@example.test).
"""


class HarnessError(RuntimeError):
    """Fatal setup/teardown error for the harness itself."""


class AnsiCapturePending(HarnessError):
    """The pane is not styled yet; ready polling may retry this transient frame."""


class ScenarioFailure(AssertionError):
    """A requested behavior assertion failed."""


class LocalHttp:
    """Small localhost-only HTTP evidence server used only with --live-effects."""

    def __init__(self) -> None:
        self.paths: list[str] = []

        owner = self

        class Handler(http.server.BaseHTTPRequestHandler):
            def do_GET(self) -> None:  # noqa: N802 - stdlib handler API
                owner.paths.append(self.path)
                body = b"leaf-link-navigation-local-ok\n"
                self.send_response(200)
                self.send_header("Content-Type", "text/plain; charset=utf-8")
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)

            def log_message(self, _format: str, *_args: object) -> None:
                return

        self.server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()

    @property
    def url(self) -> str:
        return f"http://127.0.0.1:{self.server.server_port}/leaf-link-navigation"

    def stop(self) -> None:
        self.server.shutdown()
        self.server.server_close()
        self.thread.join(timeout=2)


class ScenarioContext:
    def __init__(self, harness: "Harness", name: str) -> None:
        self.harness = harness
        self.name = name
        self.session: Optional[Session] = None
        self.assertions: list[dict[str, Any]] = []
        self.captures: list[dict[str, str]] = []

    def start(
        self,
        width: int = DEFAULT_WIDTH,
        height: int = DEFAULT_HEIGHT,
        watch: bool = False,
        ready_text: str = "First",
        editor: Optional[Path] = None,
    ) -> None:
        self.session = Session(self.harness, self.name, width, height, watch, editor)
        self.session.start(ready_text)
        self.capture("startup")

    def capture(self, label: str) -> tuple[str, str]:
        if self.session is None:
            raise HarnessError("scenario session was not started")
        text, ansi, paths = self.session.capture(label)
        self.captures.append(paths)
        return text, ansi

    def send(self, key: str, label: Optional[str] = None) -> tuple[str, str]:
        if self.session is None:
            raise HarnessError("scenario session was not started")
        self.session.send(key)
        return self.capture(label or f"after-{key}")

    def check(self, name: str, expected: Any, observed: Any, passed: bool) -> None:
        assertion = {
            "name": name,
            "expected": expected,
            "observed": observed,
            "passed": bool(passed),
        }
        self.assertions.append(assertion)
        if not passed:
            raise ScenarioFailure(
                f"{name}: expected {expected!r}, observed {observed!r}"
            )

    def status(self, text: str) -> str:
        return status_line(text)

    def wait_status(self, destination_fragment: str, index: int, label: str) -> str:
        if self.session is None:
            raise HarnessError("scenario session was not started")
        count = len(self.harness.expected_order)
        expected_index = re.compile(rf"(?<![\w/]){index}/{count}(?![\w/])")
        text = self.session.wait_for(
            lambda value: (
                expected_index.search(status_line(value)) is not None
                and destination_fragment in status_line(value)
            ),
            f"{index}/{count} and {destination_fragment!r}",
        )
        status = self.status(text)
        self.captures.append(self.session.capture(label)[2])
        self.check(
            f"{label}: selected occurrence index",
            f"{index}/{count}",
            status,
            expected_index.search(status) is not None,
        )
        self.check(
            f"{label}: standard status has no LINK prefix",
            "no LINK prefix",
            status,
            "LINK " not in status,
        )
        self.check(
            f"{label}: standard status has Leaf filename",
            "link-navigation.md",
            status,
            "link-navigation.md" in status,
        )
        self.check(
            f"{label}: destination",
            destination_fragment,
            status,
            destination_fragment in status,
        )
        return status

    def close(self) -> None:
        if self.session is not None:
            self.session.close()
            self.session = None


class Session:
    def __init__(
        self,
        harness: Harness,
        name: str,
        width: int,
        height: int,
        watch: bool = False,
        editor: Optional[Path] = None,
    ) -> None:
        self.harness = harness
        self.name = name
        self.width = width
        self.height = height
        self.session_name = f"leaf-{name}-{uuid.uuid4().hex[:8]}"
        self.watch = watch
        self.editor = editor
        self.started = False

    def tmux(self, *args: str, timeout: float = 5.0) -> subprocess.CompletedProcess[str]:
        command = [
            self.harness.tmux,
            "-L",
            self.harness.server_name,
            "-f",
            str(self.harness.tmux_conf),
            *args,
        ]
        return subprocess.run(
            command,
            check=False,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            timeout=timeout,
            env=self.harness.base_env,
        )

    def start(self, ready_text: str = "First") -> None:
        env_items = [
            f"HOME={self.harness.home}",
            f"XDG_CONFIG_HOME={self.harness.home / '.config'}",
            f"XDG_DATA_HOME={self.harness.home / '.local' / 'share'}",
            f"XDG_CACHE_HOME={self.harness.home / '.cache'}",
            f"XDG_STATE_HOME={self.harness.home / '.local' / 'state'}",
            f"LEAF_TEST_CLIPBOARD={self.harness.clipboard_path}",
        ]
        if self.editor is not None:
            env_items.append(f"LEAF_EDITOR={self.editor}")
        command = [
            "new-session",
            "-d",
            "-s",
            self.session_name,
            "-x",
            str(self.width),
            "-y",
            str(self.height),
            "--",
            "env",
            *env_items,
            str(self.harness.binary),
            *(["--watch"] if self.watch else []),
            str(self.harness.fixture_path),
        ]
        result = self.tmux(*command, timeout=10)
        if result.returncode != 0:
            raise HarnessError(
                f"tmux new-session failed: {result.stderr.strip() or result.stdout.strip()}"
            )
        self.started = True
        self.wait_for(lambda text: ready_text in text, "fixture text", timeout=8)

    def send(self, key: str) -> None:
        if not self.started:
            raise HarnessError("tmux session is not running")
        if key == "Esc":
            key = "Escape"
        args = ["send-keys", "-t", self.session_name, key]
        result = self.tmux(*args)
        if result.returncode != 0:
            raise HarnessError(
                f"tmux send-keys failed for {key!r}: {result.stderr.strip()}"
            )
        time.sleep(self.harness.settle)

    def resize(self, width: int, height: int) -> None:
        result = self.tmux(
            "resize-window",
            "-t",
            self.session_name,
            "-x",
            str(width),
            "-y",
            str(height),
        )
        if result.returncode != 0:
            raise HarnessError(f"tmux resize-window failed: {result.stderr.strip()}")
        self.width = width
        self.height = height
        time.sleep(self.harness.settle)

    def capture(self, label: str) -> tuple[str, str, dict[str, str]]:
        if not self.started:
            raise HarnessError("tmux session is not running")
        plain_result = self.tmux(
            "capture-pane",
            "-p",
            "-t",
            self.session_name,
        )
        ansi_result = self.tmux(
            "capture-pane",
            "-p",
            "-e",
            "-t",
            self.session_name,
        )
        if plain_result.returncode != 0 or ansi_result.returncode != 0:
            detail = plain_result.stderr.strip() or ansi_result.stderr.strip()
            raise HarnessError(f"tmux capture-pane failed: {detail}")
        if not ansi_style_bytes(ansi_result.stdout):
            raise AnsiCapturePending(
                "capture-pane -e returned no ANSI SGR bytes yet; "
                "waiting for a styled ready frame"
            )
        safe = safe_name(label)
        plain_path = self.harness.run_dir / f"{self.name}-{safe}.txt"
        ansi_path = self.harness.run_dir / f"{self.name}-{safe}.ansi"
        plain_path.write_text(plain_result.stdout, encoding="utf-8")
        ansi_path.write_text(ansi_result.stdout, encoding="utf-8")
        paths = {"text": str(plain_path), "ansi": str(ansi_path)}
        return plain_result.stdout, ansi_result.stdout, paths

    def wait_for(
        self,
        predicate: Callable[[str], bool],
        description: str,
        timeout: Optional[float] = None,
    ) -> str:
        deadline = time.monotonic() + (timeout or self.harness.timeout)
        last = ""
        while time.monotonic() < deadline:
            try:
                last, _, _ = self.capture(f"poll-{safe_name(description)}")
            except AnsiCapturePending:
                time.sleep(self.harness.poll_interval)
                continue
            if predicate(last):
                return last
            time.sleep(self.harness.poll_interval)
        raise ScenarioFailure(
            f"timed out waiting for {description}; observed pane tail: {last[-500:]!r}"
        )
    def close(self) -> None:
        if not self.started:
            return
        self.tmux("kill-session", "-t", self.session_name, timeout=3)

        self.started = False


def safe_name(value: str) -> str:
    cleaned = "".join(ch if ch.isalnum() or ch in "-_" else "_" for ch in value)
    return cleaned.strip("_")[:80] or "capture"


def ansi_style_bytes(value: str) -> bool:
    """Return true when a tmux `capture-pane -e` result contains an SGR sequence."""
    return re.search(r"\x1b\[[0-9;:]*m", value) is not None

INDEX_COUNT_RE = re.compile(r"(?<!\S)\d+/\d+(?!\S)")

def status_line(text: str) -> str:
    lines = text.splitlines()
    if not lines:
        return ""
    line = lines[-1].strip()
    return line if INDEX_COUNT_RE.search(line) else ""

def status_has_index(text: str, index: int, count: int) -> bool:
    return re.search(rf"(?<![\w/]){index}/{count}(?![\w/])", status_line(text)) is not None


def visible_rows(text: str, height: int) -> list[str]:
    rows = text.splitlines()
    if len(rows) < height:
        rows = [""] * (height - len(rows)) + rows
    return rows[-height:]


def write_executable(path: Path, body: str) -> None:
    path.write_text(body, encoding="utf-8")
    path.chmod(0o700)


def fixture_text(local_url: Optional[str]) -> str:
    heading_words = " ".join(["abcdefghij"] * 40)
    local_link = f"\n[Local HTTP]({local_url})\n" if local_url else ""
    return FIXTURE_TEMPLATE.format(
        long_url=LONG_URL,
        heading_words=heading_words,
        local_link=local_link,
    )


class Harness:
    def __init__(self, binary: Path, output: Path, live_effects: bool, timeout: float) -> None:
        self.binary = binary
        self.output_root = output
        self.live_effects = live_effects
        self.timeout = timeout
        self.poll_interval = 0.12
        self.settle = 0.18
        self.tmux = shutil.which("tmux") or ""
        self.server_name = f"leaf-link-nav-{os.getpid()}-{uuid.uuid4().hex[:8]}"
        self.run_dir = output / (
            "link-navigation-"
            + _datetime.datetime.now(_datetime.timezone.utc).strftime("%Y%m%dT%H%M%SZ")
            + "-"
            + uuid.uuid4().hex[:8]
        )
        self.runtime: Optional[Path] = None
        self.home: Optional[Path] = None
        self.tmux_conf: Optional[Path] = None
        self.fixture_path: Optional[Path] = None
        self.clipboard_path: Optional[Path] = None
        self.local_http: Optional[LocalHttp] = None
        self.real_clipboard: Optional[bytes] = None
        self.clipboard_read_command: Optional[str] = None
        self.clipboard_write_command: Optional[str] = None
        self.base_env: dict[str, str] = {}
        self.expected_order: list[str] = []

    def setup(self) -> None:
        if not self.tmux:
            raise HarnessError("tmux is required; install tmux or pass an environment with tmux")
        if not self.binary.is_file():
            raise HarnessError(f"binary does not exist: {self.binary}")
        if not os.access(self.binary, os.X_OK):
            raise HarnessError(f"binary is not executable: {self.binary}")
        self.output_root.mkdir(parents=True, exist_ok=True)
        self.run_dir.mkdir(parents=True, exist_ok=False)
        self.runtime = Path(tempfile.mkdtemp(prefix="runtime-", dir=self.run_dir))
        self.home = self.runtime / "home"
        (self.home / ".config").mkdir(parents=True)
        (self.home / ".local" / "share").mkdir(parents=True)
        (self.home / ".cache").mkdir(parents=True)
        (self.home / ".local" / "state").mkdir(parents=True)
        bin_dir = self.runtime / "bin"
        bin_dir.mkdir()
        self.tmux_conf = self.runtime / "tmux.conf"
        self.tmux_conf.write_text(
            "set -g status off\n"
            "set -g mouse off\n"
            "set -g history-limit 10000\n"
            "set -g default-terminal \"xterm-256color\"\n"
            "set -as terminal-features ',xterm-256color:RGB'\n",
            encoding="utf-8",
        )
        self.clipboard_path = self.runtime / "clipboard.bin"

        base_path = os.environ.get("PATH", "")
        self.base_env = dict(os.environ)
        self.base_env.pop("NO_COLOR", None)
        self.base_env.update(
            {
                "HOME": str(self.home),
                "XDG_CONFIG_HOME": str(self.home / ".config"),
                "XDG_DATA_HOME": str(self.home / ".local" / "share"),
                "XDG_CACHE_HOME": str(self.home / ".cache"),
                "XDG_STATE_HOME": str(self.home / ".local" / "state"),
                "LANG": "C.UTF-8",
                "LC_ALL": "C.UTF-8",
                "TERM": "xterm-256color",
                "COLORTERM": "truecolor",
                "CLICOLOR_FORCE": "1",
                "FORCE_COLOR": "1",
                "PATH": f"{bin_dir}{os.pathsep}{base_path}",
            }
        )

        if self.live_effects:
            self._setup_live_effects()
        else:
            self._setup_stub_clipboard(bin_dir)

        if self.live_effects:
            self.local_http = LocalHttp()
        self.fixture_path = self.runtime / "link-navigation.md"
        self.fixture_path.write_text(
            fixture_text(self.local_http.url if self.local_http else None), encoding="utf-8"
        )
        self.expected_order = [
            "https://example.test/first",
            "https://example.test/first",
            LONG_URL,
            "#local-target",
            "docs/relative.md",
            "mailto:link@example.test",
            "https://example.test/heading-late",
            "https://example.test/a",
            "https://example.test/b",
            "https://example.test/c",
        ]
        if self.local_http:
            self.expected_order.append(self.local_http.url)
        self.expected_order.append("mailto:last@example.test")
        self.default_order = list(self.expected_order)

    def _setup_stub_clipboard(self, bin_dir: Path) -> None:
        assert self.clipboard_path is not None
        writer = "#!/bin/sh\ncat > \"$LEAF_TEST_CLIPBOARD\"\n"
        reader = "#!/bin/sh\nif [ -f \"$LEAF_TEST_CLIPBOARD\" ]; then cat \"$LEAF_TEST_CLIPBOARD\"; fi\n"
        for command in ("pbcopy", "wl-copy", "xclip", "xsel", "termux-clipboard-set"):
            write_executable(bin_dir / command, writer)
        write_executable(bin_dir / "pbpaste", reader)

    def _setup_live_effects(self) -> None:
        candidates = [("pbcopy", "pbpaste"), ("wl-copy", "wl-paste")]
        for writer, reader in candidates:
            if shutil.which(writer) and shutil.which(reader):
                self.clipboard_write_command = writer
                self.clipboard_read_command = reader
                break
        if not self.clipboard_read_command or not self.clipboard_write_command:
            raise HarnessError(
                "--live-effects requires a real clipboard pair (pbcopy/pbpaste or wl-copy/wl-paste)"
            )
        read = subprocess.run(
            [self.clipboard_read_command],
            check=False,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=3,
        )
        if read.returncode != 0:
            detail = read.stderr.decode("utf-8", errors="replace").strip()
            raise HarnessError(
                f"real clipboard read failed with exit {read.returncode}: {detail}"
            )
        self.real_clipboard = read.stdout

    def read_clipboard(self) -> bytes:
        if self.live_effects:
            assert self.clipboard_read_command is not None
            result = subprocess.run(
                [self.clipboard_read_command],
                check=False,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                timeout=3,
            )
            if result.returncode != 0:
                detail = result.stderr.decode("utf-8", errors="replace").strip()
                raise HarnessError(
                    f"real clipboard read failed with exit {result.returncode}: {detail}"
                )
            return result.stdout
        assert self.clipboard_path is not None
        return self.clipboard_path.read_bytes() if self.clipboard_path.exists() else b""

    def restore_effects(self) -> None:
        if self.live_effects and self.real_clipboard is not None and self.clipboard_write_command:
            result = subprocess.run(
                [self.clipboard_write_command],
                input=self.real_clipboard,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                timeout=3,
                check=False,
            )
            if result.returncode != 0:
                raise HarnessError("failed to restore real clipboard")

    def cleanup(self) -> None:
        subprocess.run(
            [self.tmux, "-L", self.server_name, "kill-server"],
            check=False,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            timeout=3,
            env=self.base_env or None,
        ) if self.tmux else None
        if self.local_http:
            self.local_http.stop()
            self.local_http = None
        self.restore_effects()
        if self.runtime and self.runtime.exists():
            shutil.rmtree(self.runtime, ignore_errors=True)


def start_custom(
    harness: Harness,
    context: ScenarioContext,
    fixture: str,
    order: list[str],
    width: int,
    height: int,
    watch: bool = False,
    ready_text: str = "First",
) -> None:
    assert harness.fixture_path is not None
    harness.fixture_path.write_text(fixture, encoding="utf-8")
    harness.expected_order = order
    context.start(width=width, height=height, watch=watch, ready_text=ready_text)


def scenario_backward_reveal(harness: Harness, ctx: ScenarioContext) -> None:
    long_heading = " ".join(["abcdefghij"] * 40)
    fixture = f"# [First](https://first.test) {long_heading} [Late](https://late.test)\n"
    start_custom(harness, ctx, fixture, ["https://first.test", "https://late.test"], 50, 6)
    ctx.send("f", "backward-entry-first")
    ctx.wait_status("https://first", 1, "backward-first")
    ctx.send("n", "backward-entry-late")
    ctx.wait_status("https://late.test", 2, "backward-late")
    ctx.send("n", "backward-reveal-first")
    body, _ = ctx.capture("backward-first-body")
    ctx.check("backward target is visible", "#First", body, "#First" in body)


def scenario_footnote_order(harness: Harness, ctx: ScenarioContext) -> None:
    fixture = (
        "Body [Body](https://body.test) references[^second] before[^first].\n\n"
        "[^first]: First note [First note link](https://first-note.test) with extra words.\n\n"
        "[^second]: Second note [Second note link](https://second-note.test) with extra words.\n"
    )
    start_custom(
        harness,
        ctx,
        fixture,
        ["https://body.test", "https://second-note.test", "https://first-note.test"],
        80,
        12,
    )
    ctx.send("f", "footnote-entry-body")
    ctx.wait_status("https://body.test", 1, "footnote-body")
    ctx.send("n", "footnote-second")
    ctx.wait_status("https://second-note.test", 2, "footnote-second-note")
    ctx.send("n", "footnote-first")
    ctx.wait_status("https://first-note.test", 3, "footnote-first-note")
    ctx.capture("footnote-notes-body")


def scenario_shared_dispatch(harness: Harness, ctx: ScenarioContext) -> None:
    fixture = "# [First](https://first.test) and [Late](https://late.test)\n"
    start_custom(harness, ctx, fixture, ["https://first.test", "https://late.test"], 80, 12)
    ctx.send("f", "shared-entry")
    ctx.wait_status("https://first.test", 1, "shared-first")
    before, _ = ctx.capture("shared-before-normal-keys")
    before_status = status_line(before)
    assert ctx.session is not None
    before_body = visible_rows(before, ctx.session.height)[:-1]

    for key in ("h", "Left", "Right"):
        after, _ = ctx.send(key, f"shared-normal-{key}")
        after_status = status_line(after)
        after_body = visible_rows(after, ctx.session.height)[:-1]
        ctx.check(
            f"{key} keeps the normal link status",
            before_status,
            after_status,
            before_status == after_status,
        )
        ctx.check(
            f"{key} follows the normal no-op dispatcher path",
            before_body,
            after_body,
            before_body == after_body,
        )

    numbered, _ = ctx.send("l", "shared-line-numbers")
    numbered_status = status_line(numbered)
    numbered_body = visible_rows(numbered, ctx.session.height)[:-1]
    ctx.check(
        "l toggles line numbers while link mode is active",
        f"1/{len(harness.expected_order)}",
        numbered_status,
        status_has_index(numbered_status, 1, len(harness.expected_order)),
    )
    ctx.check(
        "l changes the rendered body",
        "a numbered gutter",
        numbered_body,
        numbered_body != before_body and any("│" in row for row in numbered_body),
    )
    ctx.send("l", "shared-line-numbers-off")

    ctx.send("C-q", "shared-ctrl-q")
    picker = ctx.session.wait_for(
        lambda text: "Query:" in text,
        "Ctrl+q fuzzy file picker",
    )
    ctx.captures.append(ctx.session.capture("shared-ctrl-q-picker")[2])
    ctx.check(
        "Ctrl+q queues the fuzzy picker from link mode",
        "Query:",
        picker,
        "Query:" in picker,
    )
    ctx.send("C-c", "shared-ctrl-q-close")
    ctx.wait_status("https://first.test", 1, "shared-ctrl-q-link-mode-restored")



def scenario_long_document_scrolling(harness: Harness, ctx: ScenarioContext) -> None:
    lines: list[str] = []
    for index in range(140):
        if index == 30:
            lines.append("[First](https://scroll-first.test)")
        elif index == 100:
            lines.append("[Second](https://scroll-second.test)")
        else:
            lines.append(f"document-row-{index:03d}")
    fixture = "\n".join(lines) + "\n"
    start_custom(
        harness,
        ctx,
        fixture,
        ["https://scroll-first.test", "https://scroll-second.test"],
        80,
        8,
        ready_text="document-row-000",
    )
    ctx.send("f", "long-scroll-entry")
    ctx.wait_status("https://scroll-first.test", 1, "long-scroll-first")
    assert ctx.session is not None

    def viewport(text: str) -> list[str]:
        return visible_rows(text, ctx.session.height)[:-1]

    def check_move(key: str, before: str, label: str) -> str:
        before_body = viewport(before)
        moved, _ = ctx.send(key, label)
        moved_status = status_line(moved)
        moved_body = viewport(moved)
        ctx.check(
            f"{key} keeps link mode while scrolling",
            "1/2",
            moved_status,
            status_has_index(moved_status, 1, 2),
        )
        ctx.check(
            f"{key} moves the native document viewport",
            True,
            moved_body,
            moved_body != before_body,
        )
        steady, _ = ctx.capture(f"{label}-steady")
        ctx.check(
            f"{key} does not snap back on the next frame",
            moved_body,
            viewport(steady),
            viewport(steady) == moved_body,
        )
        return moved

    current, _ = ctx.capture("long-scroll-baseline")
    for down, up in (("j", "k"), ("d", "u"), ("Down", "Up"), ("PageDown", "PageUp")):
        moved = check_move(down, current, f"long-scroll-{down}")
        returned = check_move(up, moved, f"long-scroll-{up}")
        current = returned

    for key in ("Home", "End"):
        current = check_move(key, current, f"long-scroll-{key}")

    ctx.send("n", "long-scroll-next")
    next_text = ctx.session.wait_for(
        lambda text: status_has_index(text, 2, 2)
        and "https://scroll-second.test" in status_line(text)
        and "Second" in text,
        "n reveals the next link",
    )
    ctx.captures.append(ctx.session.capture("long-scroll-next-steady")[2])
    ctx.check(
        "n explicitly reveals the next link",
        "Second",
        next_text,
        "Second" in next_text,
    )
    ctx.send("N", "long-scroll-previous")
    previous_text = ctx.session.wait_for(
        lambda text: status_has_index(text, 1, 2)
        and "https://scroll-first.test" in status_line(text)
        and "First" in text,
        "N reveals the previous link",
    )
    ctx.captures.append(ctx.session.capture("long-scroll-previous-steady")[2])
    ctx.check(
        "N explicitly reveals the previous link",
        "First",
        previous_text,
        "First" in previous_text,
    )


def scenario_narrow_wide_narrow(harness: Harness, ctx: ScenarioContext) -> None:
    long_heading = " ".join(["abcdefghij"] * 40)
    fixture = f"# [First](https://first.test) {long_heading} [Late](https://late.test)\n"
    start_custom(harness, ctx, fixture, ["https://first.test", "https://late.test"], 80, 12)
    ctx.send("f", "resize-cycle-entry")
    ctx.send("n", "resize-cycle-late")
    ctx.wait_status("https://late.test", 2, "resize-cycle-selected")
    assert ctx.session is not None
    for width, label in [(34, "narrow"), (80, "wide"), (34, "narrow-again")]:
        ctx.session.resize(width, 12)
        ctx.session.wait_for(lambda text: status_has_index(text, 2, 2), f"{label} selection")
    ctx.capture("resize-cycle-final")


def scenario_mouse_capture_off(harness: Harness, ctx: ScenarioContext) -> None:
    ctx.start()
    ctx.send("f", "mouse-off-entry")
    before = status_line(ctx.capture("mouse-off-before")[0])
    ctx.send("m", "mouse-off-toggle")
    after = status_line(ctx.capture("mouse-off-after")[0])
    ctx.check("mouse capture toggle keeps link status", "1/" + str(len(harness.expected_order)), after, status_has_index(after, 1, len(harness.expected_order)))
    ctx.check(
        "mouse capture toggle keeps selected destination",
        before,
        after,
        "https://example.test/first" in after,
    )


def scenario_watch_reload_exits(harness: Harness, ctx: ScenarioContext) -> None:
    fixture = "[first](https://watch-first.test)\n"
    start_custom(
        harness,
        ctx,
        fixture,
        ["https://watch-first.test"],
        80,
        8,
        watch=True,
        ready_text="first",
    )
    ctx.send("f", "watch-entry")
    ctx.wait_status("https://watch-first.test", 1, "watch-selected")
    assert harness.fixture_path is not None
    harness.fixture_path.write_text("watch replacement has no links\n", encoding="utf-8")
    assert ctx.session is not None
    replaced = ctx.session.wait_for(
        lambda text: not status_line(text),
        "watch replacement exits link mode",
    )
    ctx.check("watch replacement clears link mode", "no index/count status", status_line(replaced), not status_line(replaced))


def scenario_no_link_and_below_only_entry(harness: Harness, ctx: ScenarioContext) -> None:
    start_custom(harness, ctx, "plain text only\n", [], 80, 2, ready_text="plain")
    ctx.send("f", "no-link-entry")
    assert ctx.session is not None
    no_link = ctx.session.wait_for(
        lambda text: "No links in document" in text,
        "no-link feedback",
    )
    ctx.check("no-link document does not enter mode", "no index/count status", status_line(no_link), not status_line(no_link))

    ctx.close()
    start_custom(
        harness,
        ctx,
        "top text\n\n[below](https://below-only.test)\n",
        ["https://below-only.test"],
        80,
        2,
        ready_text="top",
    )
    ctx.send("f", "below-only-entry")
    ctx.wait_status("https://below-only.test", 1, "below-only-selected")


def scenario_entry_and_cycles(harness: Harness, ctx: ScenarioContext) -> None:
    ctx.start()
    ctx.send("f", "enter-first")
    ctx.wait_status("https://example.test/first", 1, "first-visible")
    ctx.send("N", "previous-wrap-last")
    ctx.wait_status("mailto:last@example.test", len(harness.expected_order), "N-at-first-wraps-last")
    ctx.send("n", "next-wrap-first")
    ctx.wait_status("https://example.test/first", 1, "n-at-last-wraps-first")
    ctx.send("n", "next-repeated-first")
    ctx.wait_status("https://example.test/first", 2, "repeated-destination-remains-distinct")


def scenario_copy_and_unsupported(harness: Harness, ctx: ScenarioContext) -> None:
    ctx.start()
    ctx.send("f", "copy-enter-first")
    ctx.wait_status("https://example.test/first", 1, "copy-selected-first")
    ctx.send("n", "copy-next-repeat")
    ctx.send("n", "copy-next-long")
    ctx.wait_status("https://example.test/long", 3, "copy-selected-long")
    before_copy, _ = ctx.capture("before-copy")
    ctx.send("Enter", "after-copy")
    assert ctx.session is not None
    copied_status = status_line(ctx.session.wait_for(lambda text: "Copied to clipboard" in status_line(text), "copy feedback"))
    ctx.check("copy reports success", "Copied to clipboard", copied_status, "Copied to clipboard" in copied_status)
    ctx.check("copy retains link mode", "3/" + str(len(harness.expected_order)), copied_status, status_has_index(copied_status, 3, len(harness.expected_order)))
    observed_clipboard = harness.read_clipboard()
    ctx.check(
        "copy preserves complete destination",
        LONG_URL,
        observed_clipboard.decode("utf-8", errors="replace"),
        observed_clipboard == LONG_URL.encode("utf-8"),
    )
    ctx.check("copy scenario had a visible pane", True, bool(before_copy.strip()), bool(before_copy.strip()))

    ctx.send("Escape", "exit-before-unsupported")
    assert ctx.session is not None
    ctx.session.wait_for(lambda text: not status_line(text), "exit before re-entry")
    ctx.send("f", "unsupported-reenter")
    for _ in range(3):
        ctx.send("n", "advance-to-anchor")
    ctx.wait_status("#local-target", 4, "unsupported-selected-anchor")
    ctx.send("o", "unsupported-open")
    assert ctx.session is not None
    unsupported_status = status_line(ctx.session.wait_for(lambda text: "Only HTTP(S) links can open" in status_line(text), "unsupported target feedback"))
    ctx.check("unsupported target refuses open", "Only HTTP(S) links can open", unsupported_status, "Only HTTP(S) links can open" in unsupported_status)
    ctx.check("unsupported target remains in link mode", "4/" + str(len(harness.expected_order)), unsupported_status, status_has_index(unsupported_status, 4, len(harness.expected_order)))


def scenario_toggle_and_clipping(harness: Harness, ctx: ScenarioContext) -> None:
    ctx.start()
    ctx.send("f", "toggle-enter")
    ctx.send("n", "toggle-repeat")
    ctx.send("n", "toggle-long")
    assert ctx.session is not None
    before, _ = ctx.capture("toggle-before-normal-keys")
    before_status = status_line(before)
    ctx.check("f status has no LINK prefix", "no LINK prefix", before_status, "LINK " not in before_status)
    ctx.check("f status has Leaf filename", "link-navigation.md", before_status, "link-navigation.md" in before_status)
    count = len(harness.expected_order)
    ctx.check("f enters link mode", f"3/{count}", before_status, status_has_index(before, 3, count))
    ctx.check(
        "long URL status keeps visible prefix",
        "https://example.test/long",
        before_status,
        "https://example.test/long" in before_status,
    )
    ctx.check("long URL status clips the tail", LONG_URL, before_status, LONG_URL not in before_status)
    ctx.check(
        "long URL status clips before destination tail",
        "-wrapped-link-destination",
        before_status,
        "-wrapped-link-destination" not in before_status,
    )
    ctx.check(
        "long URL status does not wrap onto another row",
        "-wrapped-link-destination",
        before,
        "-wrapped-link-destination" not in before,
    )
    before_body = visible_rows(before, ctx.session.height)[:-1]
    for key in ("h", "Left", "Right"):
        after, _ = ctx.send(key, f"toggle-normal-{key}")
        after_status = status_line(after)
        after_body = visible_rows(after, ctx.session.height)[:-1]
        ctx.check(
            f"{key} keeps link status through normal dispatch",
            before_status,
            after_status,
            before_status == after_status,
        )
        ctx.check(f"{key} keeps the normal viewport", before_body, after_body, before_body == after_body)

    ctx.send("Enter", "toggle-copy")
    copied_status = status_line(
        ctx.session.wait_for(lambda text: "Copied to clipboard" in status_line(text), "long URL copy feedback")
    )
    ctx.check("long URL copy reports success", "Copied to clipboard", copied_status, "Copied to clipboard" in copied_status)
    ctx.check("copy status has no LINK prefix", "no LINK prefix", copied_status, "LINK " not in copied_status)
    ctx.check(
        "long URL copy retains link index",
        f"3/{count}",
        copied_status,
        status_has_index(copied_status, 3, count),
    )
    observed_clipboard = harness.read_clipboard()
    ctx.check(
        "long URL copy preserves complete destination",
        LONG_URL,
        observed_clipboard.decode("utf-8", errors="replace"),
        observed_clipboard == LONG_URL.encode("utf-8"),
    )

    ctx.send("f", "toggle-exit")
    exited = ctx.session.wait_for(lambda text: not status_line(text), "lowercase f link mode exit")
    ctx.check("lowercase f exits link mode", "no index/count status", status_line(exited), not status_line(exited))


def scenario_esc_preserves_view(harness: Harness, ctx: ScenarioContext) -> None:
    ctx.start()
    for index in range(10):
        ctx.send("j", f"scroll-{index + 1}")
    ctx.capture("before-link-mode")
    ctx.send("f", "esc-enter")
    in_mode, _ = ctx.capture("in-link-mode")
    ctx.send("Escape", "esc-exit")
    assert ctx.session is not None
    after = ctx.session.wait_for(lambda text: not status_line(text), "link mode exit")
    ctx.capture("after-link-mode")
    before_body = visible_rows(in_mode, DEFAULT_HEIGHT)[:-1]
    after_body = visible_rows(after, DEFAULT_HEIGHT)[:-1]
    ctx.check("Esc preserves body viewport", before_body, after_body, before_body == after_body)
    ctx.check("Esc exits link mode", "no index/count status", status_line(after), not status_line(after))
    ctx.send("f", "ctrl-c-enter")
    assert ctx.session is not None
    ctx.session.wait_for(lambda text: bool(status_line(text)), "Ctrl+c link mode entry")
    ctrl_c_text, _ = ctx.send("C-c", "ctrl-c-exit")
    ctx.check(
        "Ctrl+c exits link mode",
        "no index/count status",
        status_line(ctrl_c_text),
        not status_line(ctrl_c_text),
    )


def scenario_search_f_is_text(harness: Harness, ctx: ScenarioContext) -> None:
    ctx.start()
    ctx.send("C-f", "search-open-control-f")
    search_text, _ = ctx.send("f", "search-lowercase-f-text")
    search_status = status_line(search_text)
    draft_status = visible_rows(search_text, DEFAULT_HEIGHT)[-1]
    ctx.check("Ctrl+f opens ordinary search", "/f", draft_status, "/f" in draft_status)
    ctx.check("lowercase f remains search input", "/f", draft_status, "/f" in draft_status)
    ctx.check(
        "search input does not enter link mode",
        "no index/count status",
        search_status,
        not search_status,
    )
    ctx.send("Escape", "search-close")

    ctx.send("f", "link-search-entry")
    ctx.wait_status("https://example.test/first", 1, "link-search-selected")
    link_search_text, _ = ctx.send("C-f", "link-search-open-control-f")
    ctx.check(
        "Ctrl+f exits link mode into search",
        "no index/count status",
        status_line(link_search_text),
        not status_line(link_search_text),
    )
    link_search_input, _ = ctx.send("f", "link-search-lowercase-f-text")
    link_search_draft = visible_rows(link_search_input, DEFAULT_HEIGHT)[-1]
    ctx.check(
        "Ctrl+f input is ordinary search text",
        "/f",
        link_search_draft,
        "/f" in link_search_draft,
    )
    ctx.check(
        "search text stays outside link mode",
        "no index/count status",
        status_line(link_search_input),
        not status_line(link_search_input),
    )
    ctx.send("Escape", "link-search-close")

    ctx.send("f", "link-goto-entry")
    ctx.wait_status("https://example.test/first", 1, "link-goto-selected")
    goto_text, _ = ctx.send(":", "link-goto-open")
    goto_draft = visible_rows(goto_text, DEFAULT_HEIGHT)[-1]
    ctx.check(
        ": exits link mode into goto-line input",
        ":",
        goto_draft,
        ":" in goto_draft,
    )
    ctx.check(
        "goto-line input has no link status",
        "no index/count status",
        status_line(goto_text),
        not status_line(goto_text),
    )
    ctx.send("Escape", "link-goto-close")
    uppercase_text, _ = ctx.send("F", "uppercase-F-outside-linkmode")
    ctx.check(
        "uppercase F does not enter link mode",
        "no index/count status",
        status_line(uppercase_text),
        not status_line(uppercase_text),
    )

def scenario_editor_ctrl_e(harness: Harness, ctx: ScenarioContext) -> None:
    assert harness.runtime is not None
    assert harness.fixture_path is not None
    editor_stub = (harness.runtime / "code").resolve()
    editor_log = harness.runtime / "editor-argv.txt"
    write_executable(
        editor_stub,
        "#!/bin/sh\n"
        f"printf '%s\\n' \"$@\" > {shlex.quote(str(editor_log))}\n",
    )
    ctx.start(editor=editor_stub)
    ctx.send("f", "editor-entry")
    ctx.wait_status("https://example.test/first", 1, "editor-selected")
    ctx.send("C-e", "editor-ctrl-e")
    assert ctx.session is not None
    editor_status = ctx.session.wait_for(
        lambda text: status_has_index(text, 1, len(harness.expected_order)),
        "Ctrl+e retains link mode",
    )
    ctx.captures.append(ctx.session.capture("editor-ctrl-e-status")[2])
    ctx.check(
        "Ctrl+e keeps link mode during editor dispatch",
        f"1/{len(harness.expected_order)}",
        status_line(editor_status),
        status_has_index(editor_status, 1, len(harness.expected_order)),
    )

    deadline = time.monotonic() + harness.timeout
    observed: list[str] = []
    while time.monotonic() < deadline:
        if editor_log.exists():
            observed = editor_log.read_text(encoding="utf-8").splitlines()
            if observed:
                break
        time.sleep(harness.poll_interval)
    observed_paths = [str(Path(argument).resolve()) for argument in observed]
    expected_paths = [str(harness.fixture_path.resolve())]
    ctx.check(
        "Ctrl+e sends the fixture path to the private code stub",
        expected_paths,
        observed_paths,
        observed_paths == expected_paths,
    )





def scenario_resize_retains_selection(harness: Harness, ctx: ScenarioContext) -> None:
    ctx.start()
    ctx.send("f", "resize-enter")
    ctx.send("n", "resize-repeat")
    ctx.send("n", "resize-long")
    count = len(harness.expected_order)
    before = ctx.status(ctx.capture("resize-before")[0])
    ctx.check(
        "resize starts with long URL selected",
        "https://example.test/long",
        before,
        "https://example.test/long" in before,
    )
    assert ctx.session is not None
    ctx.session.resize(70, DEFAULT_HEIGHT)
    resized = ctx.session.wait_for(
        lambda text: status_has_index(text, 3, count)
        and "https://example.test/long" in status_line(text),
        "selected long URL after resize",
    )
    ctx.captures.append(ctx.session.capture("resize-after")[2])
    resized_status = status_line(resized)
    ctx.check(
        "resize retains occurrence",
        "https://example.test/long",
        resized_status,
        "https://example.test/long" in resized_status,
    )
    ctx.check(
        "resize retains link mode",
        f"3/{count}",
        resized_status,
        status_has_index(resized_status, 3, count),
    )


def scenario_wrapped_table_heading(harness: Harness, ctx: ScenarioContext) -> None:
    ctx.start(width=50, height=12)
    ctx.send("f", "geometry-enter")
    ctx.wait_status("https://", 1, "geometry-first")
    assert ctx.session is not None
    count = len(harness.expected_order)
    for index in range(2, 8):
        ctx.send("n", f"geometry-next-{index}")
        ctx.session.wait_for(
            lambda text, index=index: status_has_index(text, index, count),
            f"narrow geometry selection {index}/{count}",
        )
    heading_text, _ = ctx.capture("long-heading-body")
    ctx.check("long heading target is visible after reveal", "TargetZ", heading_text, "TargetZ" in heading_text)
    ctx.wait_status("https://", 7, "long-heading-revealed")
    for label, index in [("a", 8), ("b", 9), ("c", 10)]:
        ctx.send("n", f"table-{label}")
        ctx.wait_status("https://", index, f"table-{label}-visual-order")
        ctx.send("Enter", f"table-{label}-copy-clipped-destination")
        ctx.session.wait_for(lambda text: "Copied" in status_line(text), "table destination copied")
        expected = f"https://example.test/{label}"
        copied = harness.read_clipboard().decode("utf-8")
        ctx.check(f"narrow table {label} destination", expected, copied, copied == expected)


def scenario_live_open(harness: Harness, ctx: ScenarioContext) -> None:
    if not harness.live_effects or harness.local_http is None:
        raise ScenarioFailure("live-effects scenario requested without live setup")
    ctx.start()
    local_index = harness.expected_order.index(harness.local_http.url) + 1
    ctx.send("f", "live-enter")
    for index in range(2, local_index + 1):
        ctx.send("n", f"live-next-{index}")
    ctx.wait_status("127.0.0.1", local_index, "live-local-http-selected")
    ctx.send("o", "live-open-local")
    open_text = (
        ctx.session.wait_for(
            lambda text: "Open requested" in status_line(text), "local open feedback"
        )
        if ctx.session
        else ""
    )
    ctx.captures.append(ctx.session.capture("live-open-feedback")[2]) if ctx.session else None
    open_status = status_line(open_text)
    ctx.check("local HTTP open reports request", "Open requested", open_status, "Open requested" in open_status)
    ctx.check(
        "local HTTP open retains mode",
        f"{local_index}/{len(harness.expected_order)}",
        open_status,
        status_has_index(open_status, local_index, len(harness.expected_order)),
    )
    deadline = time.monotonic() + max(harness.timeout, 15.0)
    while time.monotonic() < deadline and "/leaf-link-navigation" not in harness.local_http.paths:
        time.sleep(harness.poll_interval)
    ctx.check(
        "browser reaches controlled localhost endpoint",
        "/leaf-link-navigation",
        list(harness.local_http.paths),
        "/leaf-link-navigation" in harness.local_http.paths,
    )


def scenario_quit_from_link_mode(harness: Harness, ctx: ScenarioContext) -> None:
    def wait_for_exit(key: str, label: str) -> None:
        assert ctx.session is not None
        result = ctx.session.tmux(
            "set-option",
            "-w",
            "-t",
            ctx.session.session_name,
            "remain-on-exit",
            "on",
        )
        if result.returncode != 0:
            raise HarnessError(result.stderr.strip())
        ctx.capture(f"{label}-before-exit")
        ctx.session.send(key)
        deadline = time.monotonic() + harness.timeout
        observed = ""
        while time.monotonic() < deadline:
            result = ctx.session.tmux(
                "display-message",
                "-p",
                "-t",
                ctx.session.session_name,
                "#{pane_dead} #{pane_dead_status}",
            )
            observed = result.stdout.strip()
            if observed.startswith("1 "):
                break
            time.sleep(harness.poll_interval)
        ctx.check(f"{key} exits Leaf cleanly from link mode", "1 0", observed, observed == "1 0")
        ctx.close()

    for key in ("q", "Q"):
        ctx.start()
        ctx.send("f", f"quit-{key}-enter")
        ctx.wait_status("https://example.test/first", 1, f"quit-{key}-selected")
        wait_for_exit(key, f"quit-{key}")

    ctx.start()
    ctx.send("f", "quit-ctrl-q-enter")
    ctx.wait_status("https://example.test/first", 1, "quit-ctrl-q-selected")
    ctx.send("C-q", "quit-ctrl-q-picker")
    assert ctx.session is not None
    picker = ctx.session.wait_for(lambda text: "Query:" in text, "Ctrl+q fuzzy picker")
    ctx.captures.append(ctx.session.capture("quit-ctrl-q-picker-ready")[2])
    ctx.check(
        "Ctrl+q opens the fuzzy picker instead of quitting",
        "Query:",
        picker,
        "Query:" in picker,
    )
    ctx.send("C-c", "quit-ctrl-q-close")
    ctx.wait_status("https://example.test/first", 1, "quit-ctrl-q-link-mode-restored")
    ctx.close()

    ctx.start()
    ctx.send("f", "quit-alt-q-enter")
    ctx.wait_status("https://example.test/first", 1, "quit-alt-q-selected")
    wait_for_exit("M-q", "quit-alt-q")

SCENARIOS: list[tuple[str, Callable[[Harness, ScenarioContext], None], bool]] = [
    ("quit-from-link-mode", scenario_quit_from_link_mode, False),
    ("mouse-capture-off", scenario_mouse_capture_off, False),
    ("watch-reload-exits", scenario_watch_reload_exits, False),
    ("no-link-below-only-entry", scenario_no_link_and_below_only_entry, False),
    ("backward-reveal", scenario_backward_reveal, False),
    ("footnote-order", scenario_footnote_order, False),
    ("shared-dispatch", scenario_shared_dispatch, False),
    ("long-document-scrolling", scenario_long_document_scrolling, False),
    ("editor-ctrl-e", scenario_editor_ctrl_e, False),
    ("narrow-wide-narrow", scenario_narrow_wide_narrow, False),
    ("entry-and-cycles", scenario_entry_and_cycles, False),
    ("copy-and-unsupported", scenario_copy_and_unsupported, False),
    ("f-toggle-clipping", scenario_toggle_and_clipping, False),
    ("esc-preserves-view", scenario_esc_preserves_view, False),
    ("search-ctrl-f-and-lowercase-f", scenario_search_f_is_text, False),
    ("resize-retains-selection", scenario_resize_retains_selection, False),
    ("wrapped-table-heading", scenario_wrapped_table_heading, False),
    ("live-local-open", scenario_live_open, True),
]


def run_scenario(harness: Harness, name: str, function: Callable[[Harness, ScenarioContext], None]) -> dict[str, Any]:
    started = time.monotonic()
    context = ScenarioContext(harness, name)
    error: Optional[str] = None
    passed = False
    try:
        assert harness.fixture_path is not None
        harness.fixture_path.write_text(
            fixture_text(harness.local_http.url if harness.local_http else None), encoding="utf-8"
        )
        harness.expected_order = list(harness.default_order)
        function(harness, context)
        passed = True
    except Exception as exc:  # evidence records the concrete assertion/setup failure
        error = f"{type(exc).__name__}: {exc}"
        try:
            context.capture("failure")
        except Exception as capture_exc:
            error += f"; capture failed: {capture_exc}"
    finally:
        context.close()
    return {
        "name": name,
        "passed": passed,
        "failure": error,
        "assertions": context.assertions,
        "captures": context.captures,
        "effect_label": "live-local-http-and-real-clipboard" if harness.live_effects else "stub-clipboard-no-live-effects",
        "duration_seconds": round(time.monotonic() - started, 3),
    }


def parse_args(argv: Optional[list[str]] = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=Path, help="leaf executable to run")
    parser.add_argument("--output", required=True, type=Path, help="directory for smoke evidence")
    parser.add_argument("--live-effects", action="store_true", help="use real clipboard and localhost-only open evidence")
    parser.add_argument("--timeout", type=float, default=DEFAULT_TIMEOUT, help="per-observation timeout in seconds")
    return parser.parse_args(argv)


def main(argv: Optional[list[str]] = None) -> int:
    args = parse_args(argv)
    binary = args.binary.expanduser().resolve()
    output = args.output.expanduser().resolve()
    harness = Harness(binary, output, args.live_effects, args.timeout)
    results: dict[str, Any] = {
        "harness": "scripts/test-link-navigation.py",
        "binary": str(binary),
        "output_root": str(output),
        "live_effects_requested": bool(args.live_effects),
        "tests_or_gates_run_by_harness": False,
        "tmux_server_isolation": harness.server_name,
        "scenarios": [],
    }
    exit_code = 1
    try:
        harness.setup()
        results.update(
            {
                "run_directory": str(harness.run_dir),
                "fixture": str(harness.fixture_path),
                "expected_navigation_order": harness.expected_order,
                "clipboard_effect_label": "real-save-restore" if args.live_effects else "temporary-stub",
                "ansi_capture_requires_sgr": True,
                "inherited_no_color_removed": "NO_COLOR" not in harness.base_env,
                "terminal_color_environment": {
                    "TERM": harness.base_env.get("TERM"),
                    "COLORTERM": harness.base_env.get("COLORTERM"),
                },
            }
        )
        for name, function, live_only in SCENARIOS:
            if live_only and not args.live_effects:
                results["scenarios"].append(
                    {
                        "name": name,
                        "passed": None,
                        "skipped": True,
                        "reason": "requires --live-effects",
                        "effect_label": "not-run",
                    }
                )
                continue
            scenario = run_scenario(harness, name, function)
            results["scenarios"].append(scenario)
        exit_code = 0 if all(item.get("passed") is not False for item in results["scenarios"]) else 1
    except Exception as exc:
        results["setup_failure"] = f"{type(exc).__name__}: {exc}"
        exit_code = 2
    finally:
        try:
            harness.cleanup()
        except Exception as exc:
            results["cleanup_failure"] = f"{type(exc).__name__}: {exc}"
            exit_code = max(exit_code, 2)
        if harness.run_dir.exists():
            results["exit_code"] = exit_code
            result_path = harness.run_dir / "results.json"
            result_path.write_text(json.dumps(results, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
            print(f"link-navigation smoke evidence: {result_path}")
    return exit_code


if __name__ == "__main__":
    raise SystemExit(main())
