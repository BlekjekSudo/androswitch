"""Small stdio MCP client and generic real-APK endurance probe.

The existing Comprix script is intentionally application-specific. This
module provides the application-neutral counterpart used for pinned launchable
APKs: start one real ``rd mcp`` process, install and launch the APK, then
exercise observation, UI-tree, screenshot, virtual-time, app-info, and runtime
health tools repeatedly in the same session.

Only Python's standard library is used.
"""

from __future__ import annotations

import base64
import hashlib
import json
import os
import selectors
import signal
import subprocess
import time
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Mapping


class McpError(RuntimeError):
    """Raised when the MCP process or a tool call fails."""


@dataclass(slots=True)
class McpEnduranceResult:
    ok: bool
    cycles_completed: int = 0
    tool_names: list[str] = field(default_factory=list)
    calls: dict[str, int] = field(default_factory=dict)
    screenshot_sha256: list[str] = field(default_factory=list)
    observe_sha256: list[str] = field(default_factory=list)
    ui_tree_sha256: list[str] = field(default_factory=list)
    initial_heap_bytes: int | None = None
    final_heap_bytes: int | None = None
    peak_heap_bytes: int | None = None
    error: str | None = None
    stderr_path: str | None = None

    def metrics(self) -> dict[str, Any]:
        return {
            "cycles_completed": self.cycles_completed,
            "tool_count": len(self.tool_names),
            "tool_names": self.tool_names,
            "calls": self.calls,
            "screenshot_sha256": self.screenshot_sha256,
            "observe_sha256": self.observe_sha256,
            "ui_tree_sha256": self.ui_tree_sha256,
            "initial_heap_bytes": self.initial_heap_bytes,
            "final_heap_bytes": self.final_heap_bytes,
            "peak_heap_bytes": self.peak_heap_bytes,
            "error": self.error,
            "stderr_path": self.stderr_path,
        }


def _canonical_sha256(value: Any) -> str:
    if isinstance(value, bytes):
        return hashlib.sha256(value).hexdigest()
    text = json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":"))
    return hashlib.sha256(text.encode("utf-8", "replace")).hexdigest()


def _first_text(result: Mapping[str, Any]) -> str:
    for item in result.get("content", []):
        if isinstance(item, Mapping) and item.get("type") == "text":
            return str(item.get("text", ""))
    return ""


def _text_as_json_or_text(result: Mapping[str, Any]) -> Any:
    text = _first_text(result)
    try:
        return json.loads(text) if text else None
    except json.JSONDecodeError:
        return text


class McpStdioClient:
    def __init__(
        self,
        binary: Path,
        *,
        cwd: Path,
        env: Mapping[str, str],
        stderr_path: Path,
        timeout_seconds: float = 120.0,
    ) -> None:
        self.timeout_seconds = timeout_seconds
        self._seq = 0
        self._pending = b""
        self.calls: dict[str, int] = {}
        stderr_path.parent.mkdir(parents=True, exist_ok=True)
        self._stderr = stderr_path.open("wb")
        try:
            self._process = subprocess.Popen(
                [str(binary), "mcp"],
                cwd=str(cwd),
                stdin=subprocess.PIPE,
                stdout=subprocess.PIPE,
                stderr=self._stderr,
                env=dict(env),
                bufsize=0,
                start_new_session=True,
            )
        except OSError as exc:
            self._stderr.close()
            raise McpError(f"could not start MCP server: {exc}") from exc
        if self._process.stdin is None or self._process.stdout is None:
            self.close()
            raise McpError("MCP server did not expose stdio pipes")
        self._selector = selectors.DefaultSelector()
        self._selector.register(self._process.stdout, selectors.EVENT_READ)

    def close(self) -> None:
        selector = getattr(self, "_selector", None)
        if selector is not None:
            try:
                selector.close()
            except Exception:
                pass
        process = getattr(self, "_process", None)
        if process is not None:
            try:
                if process.stdin is not None:
                    process.stdin.close()
            except Exception:
                pass
            try:
                process.wait(timeout=5)
            except Exception:
                try:
                    os.killpg(process.pid, signal.SIGKILL)
                except (ProcessLookupError, PermissionError, OSError):
                    pass
                try:
                    process.wait(timeout=5)
                except Exception:
                    pass
            try:
                if process.stdout is not None:
                    process.stdout.close()
            except Exception:
                pass
        try:
            self._stderr.close()
        except Exception:
            pass

    def __enter__(self) -> "McpStdioClient":
        return self

    def __exit__(self, *_exc: object) -> None:
        self.close()

    def _write_json(self, value: Mapping[str, Any]) -> None:
        if self._process.poll() is not None:
            raise McpError(f"MCP server exited with code {self._process.returncode}")
        assert self._process.stdin is not None
        payload = (json.dumps(value, ensure_ascii=False) + "\n").encode("utf-8")
        try:
            self._process.stdin.write(payload)
            self._process.stdin.flush()
        except (BrokenPipeError, OSError) as exc:
            raise McpError(f"could not write to MCP server: {exc}") from exc

    def notify(self, method: str) -> None:
        self._write_json({"jsonrpc": "2.0", "method": method})

    def request(self, method: str, params: Mapping[str, Any] | None = None) -> Mapping[str, Any]:
        self._seq += 1
        request_id = self._seq
        self._write_json(
            {
                "jsonrpc": "2.0",
                "id": request_id,
                "method": method,
                "params": dict(params or {}),
            }
        )
        deadline = time.monotonic() + self.timeout_seconds
        assert self._process.stdout is not None
        while True:
            while b"\n" not in self._pending:
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    raise McpError(f"MCP request timed out: {method}")
                if not self._selector.select(remaining):
                    raise McpError(f"MCP request timed out: {method}")
                chunk = os.read(self._process.stdout.fileno(), 65536)
                if not chunk:
                    raise McpError(
                        f"MCP server closed stdout during {method} (exit={self._process.poll()})"
                    )
                self._pending += chunk
            line, self._pending = self._pending.split(b"\n", 1)
            try:
                response = json.loads(line.decode("utf-8", "replace"))
            except json.JSONDecodeError:
                continue
            if not isinstance(response, Mapping) or response.get("id") != request_id:
                continue
            if response.get("error") is not None:
                raise McpError(f"MCP {method} returned a protocol error: {response['error']}")
            result = response.get("result")
            if not isinstance(result, Mapping):
                raise McpError(f"MCP {method} returned no object result")
            return result

    def call_tool(self, name: str, arguments: Mapping[str, Any] | None = None) -> Mapping[str, Any]:
        result = self.request(
            "tools/call",
            {"name": name, "arguments": dict(arguments or {})},
        )
        self.calls[name] = self.calls.get(name, 0) + 1
        if result.get("isError"):
            detail = _first_text(result)[:1200] or "tool reported isError"
            raise McpError(f"MCP tool {name} failed: {detail}")
        return result


def _heap_bytes(health: Any) -> int | None:
    if not isinstance(health, Mapping):
        return None
    value = health.get("heap_used_bytes")
    return int(value) if isinstance(value, (int, float)) else None


def _peak_heap_bytes(health: Any) -> int | None:
    if not isinstance(health, Mapping):
        return None
    value = health.get("heap_peak_bytes")
    return int(value) if isinstance(value, (int, float)) else None


def _screenshot_sha256(client: McpStdioClient) -> str:
    result = client.call_tool("screenshot")
    for item in result.get("content", []):
        if not isinstance(item, Mapping):
            continue
        if item.get("type") == "text" and "draw pass failed" in str(item.get("text", "")):
            raise McpError("screenshot reported a draw-pass failure")
        if item.get("type") != "image":
            continue
        raw = base64.b64decode(str(item.get("data", "")), validate=True)
        if not raw.startswith(b"\x89PNG\r\n\x1a\n"):
            raise McpError("screenshot did not return a PNG")
        return hashlib.sha256(raw).hexdigest()
    raise McpError("screenshot returned no image content")


def run_generic_apk_endurance(
    *,
    binary: Path,
    apk: Path,
    repo: Path,
    output_dir: Path,
    cycles: int,
    timeout_seconds: float = 120.0,
) -> McpEnduranceResult:
    """Exercise one launchable APK in one persistent MCP session.

    The user interface is allowed to evolve under virtual time, so changing
    hashes are recorded as evidence rather than treated as failures here.
    Cross-revision frame equivalence belongs to the A/B campaign. Tool errors,
    process exits, hangs, malformed screenshots, and missing launch state are
    failures.
    """

    output_dir.mkdir(parents=True, exist_ok=True)
    stderr_path = output_dir / "mcp-stderr.log"
    result = McpEnduranceResult(ok=False, stderr_path=str(stderr_path))
    if cycles < 1:
        result.error = f"cycles must be >= 1, got {cycles}"
        return result

    env = dict(os.environ)
    env.update(
        {
            "RD_MCP_SANDBOX": os.pathsep.join(
                [str(repo.resolve()), str(apk.resolve().parent), str(output_dir.resolve())]
            ),
            # Endurance must exercise the real Engine, not a presented
            # snapshot. GC and JIT stay enabled in the all-features binary.
            "RD_SNAPSHOT": "off",
            "RD_GC": "on",
            "RD_JIT": "on",
        }
    )

    client: McpStdioClient | None = None
    try:
        client = McpStdioClient(
            binary,
            cwd=repo,
            env=env,
            stderr_path=stderr_path,
            timeout_seconds=timeout_seconds,
        )
        client.request(
            "initialize",
            {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {"name": "rd-sentinel", "version": "1.0.0"},
            },
        )
        client.notify("notifications/initialized")
        listing = client.request("tools/list")
        tools = listing.get("tools", []) if isinstance(listing.get("tools"), list) else []
        result.tool_names = sorted(
            str(tool.get("name")) for tool in tools if isinstance(tool, Mapping) and tool.get("name")
        )
        required = {"install", "launch", "observe", "ui_tree", "screenshot", "runtime_health"}
        missing = sorted(required - set(result.tool_names))
        if missing:
            raise McpError(f"MCP tool list is missing endurance tools: {', '.join(missing)}")

        client.call_tool("install", {"apk": str(apk.resolve())})
        client.call_tool("launch", {"observe": True})
        initial_health = _text_as_json_or_text(client.call_tool("runtime_health"))
        result.initial_heap_bytes = _heap_bytes(initial_health)
        result.peak_heap_bytes = _peak_heap_bytes(initial_health)
        result.screenshot_sha256.append(_screenshot_sha256(client))

        for cycle in range(cycles):
            client.call_tool("wait", {"ms": 16})
            observed = _text_as_json_or_text(
                client.call_tool("observe", {"format": "json", "diff": False})
            )
            result.observe_sha256.append(_canonical_sha256(observed))
            ui_tree = _first_text(client.call_tool("ui_tree", {"format": "xml"}))
            if not ui_tree.strip():
                raise McpError("ui_tree returned an empty tree during endurance")
            result.ui_tree_sha256.append(_canonical_sha256(ui_tree))
            client.call_tool("app_info")
            if cycle == cycles - 1 or (cycle + 1) % 10 == 0:
                result.screenshot_sha256.append(_screenshot_sha256(client))
            health = _text_as_json_or_text(client.call_tool("runtime_health"))
            result.final_heap_bytes = _heap_bytes(health)
            peak = _peak_heap_bytes(health)
            if peak is not None:
                result.peak_heap_bytes = peak
            if isinstance(health, Mapping) and health.get("running") is False:
                raise McpError("runtime_health reports that the app stopped during endurance")
            result.cycles_completed = cycle + 1

        result.ok = True
    except Exception as exc:  # The campaign records this as a failed check.
        result.error = str(exc)[:2000]
    finally:
        if client is not None:
            result.calls = dict(client.calls)
            client.close()
    return result
