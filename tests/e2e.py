#!/usr/bin/env python3
"""End-to-end test of pcrc-server's MCP tools and admin API.

Usage:
    # terminal 1: start the server on a spare port
    pcrc-server --port 8091
    # terminal 2:
    PCRC_BASE=http://127.0.0.1:8091 python3 tests/e2e.py

The test is non-destructive: it never shuts down, suspends or reboots the
machine, restores the clipboard, and restores the token setting it toggles.
It uses a headless Chrome session, LibreOffice and the systemd "dbus" service
when available.
"""
import json
import os
import shutil
import struct
import subprocess
import sys
import urllib.error
import urllib.request

BASE = os.environ.get("PCRC_BASE", "http://127.0.0.1:8091")
MCP = BASE + "/mcp"
BIN = os.environ.get("PCRC_SERVER") or shutil.which("pcrc-server") or "pcrc-server"
WORK = os.environ.get("PCRC_WORK", "/tmp/pcrc-test")

results = []


def check(name, ok, detail=""):
    results.append((name, ok, detail))
    print(("PASS " if ok else "FAIL ") + name + ((" :: " + str(detail)[:400]) if detail else ""))


def post_mcp(payload, token=None):
    data = json.dumps(payload).encode()
    req = urllib.request.Request(MCP, data=data, headers={"Content-Type": "application/json"})
    if token:
        req.add_header("Authorization", "Bearer " + token)
    try:
        with urllib.request.urlopen(req, timeout=120) as r:
            return r.status, json.loads(r.read().decode())
    except urllib.error.HTTPError as e:
        body = e.read().decode()
        try:
            body = json.loads(body)
        except Exception:
            pass
        return e.code, body


_next_id = [0]


def rpc(method, params=None, token=None):
    _next_id[0] += 1
    return post_mcp({"jsonrpc": "2.0", "id": _next_id[0], "method": method, "params": params or {}}, token)


def tool(name, args=None):
    status, body = rpc("tools/call", {"name": name, "arguments": args or {}})
    if status != 200 or "result" not in body:
        return None, f"rpc error: {body}"
    res = body["result"]
    texts = [c.get("text", "") for c in res.get("content", []) if c.get("type") == "text"]
    parsed = None
    if texts:
        try:
            parsed = json.loads(texts[0])
        except Exception:
            parsed = texts[0]
    return {"parsed": parsed, "raw": res, "isError": res.get("isError", False)}, None


def admin(path):
    with urllib.request.urlopen(BASE + path, timeout=20) as r:
        return json.loads(r.read().decode())


def admin_put(config):
    data = json.dumps(config).encode()
    req = urllib.request.Request(BASE + "/admin/config", data=data, method="PUT",
                                 headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(req, timeout=20) as r:
        return json.loads(r.read().decode())


def main():
    os.makedirs(WORK, exist_ok=True)

    # 1. initialize
    status, body = rpc("initialize", {"protocolVersion": "2025-06-18",
                                       "capabilities": {}, "clientInfo": {"name": "pcrc-test", "version": "1"}})
    proto = body.get("result", {}).get("protocolVersion") if isinstance(body, dict) else None
    check("initialize", status == 200 and proto == "2025-06-18", body)

    # 2. tools/list
    status, body = rpc("tools/list")
    tools = [t["name"] for t in body.get("result", {}).get("tools", [])]
    expected = ["pc_screenshot", "pc_get_screen_state", "pc_mouse_click", "pc_shell_exec",
                "pc_browser_open", "pc_browser_get_content", "pc_browser_eval", "pc_fetch_page",
                "pc_doc_read", "pc_doc_convert", "pc_doc_ocr", "pc_agent_list", "pc_agent_run",
                "pc_power", "pc_admin_exec", "pc_service", "pc_terminal_run"]
    missing = [e for e in expected if e not in tools]
    check("tools/list count=54", len(tools) == 54, f"got {len(tools)}")
    check("tools/list has all new tools", not missing, missing)

    # 3. system_info
    out, err = tool("pc_system_info")
    info = out["parsed"] if out else None
    check("system_info", out and not out["isError"] and info.get("os", {}).get("hostname"),
          (info or {}).get("os") if isinstance(info, dict) else err)

    # 4. screenshot
    out, err = tool("pc_screenshot", {"max_width": 800})
    ok = False
    dims = None
    if out and not out["isError"]:
        imgs = [c for c in out["raw"]["content"] if c.get("type") == "image"]
        if imgs:
            import base64
            data = base64.b64decode(imgs[0]["data"])
            path = os.path.join(WORK, "shot.png")
            open(path, "wb").write(data)
            w, h = struct.unpack(">II", data[16:24])
            dims = (w, h)
            ok = data[:8] == b"\x89PNG\r\n\x1a\n" and w > 0
            check("screenshot backend", True, out["parsed"].get("backend") if isinstance(out["parsed"], dict) else "")
    check("screenshot png", ok, dims or err)

    # 5. screen state with windows
    out, err = tool("pc_get_screen_state", {"include_windows": True, "include_screenshot": False})
    st = out["parsed"] if out else {}
    check("get_screen_state", out and not out["isError"] and isinstance(st, dict) and "session" in st,
          (st.get("screenshot_error") if isinstance(st, dict) else err))

    # 6. clipboard roundtrip (restore afterwards)
    prev_out, _ = tool("pc_get_clipboard")
    prev = prev_out["parsed"].get("text", "") if prev_out and isinstance(prev_out["parsed"], dict) else ""
    out, err = tool("pc_set_clipboard", {"text": "pcrc clipboard test"})
    back, _ = tool("pc_get_clipboard")
    got = back["parsed"].get("text") if back and isinstance(back["parsed"], dict) else None
    check("clipboard set/get", out and not out["isError"] and got == "pcrc clipboard test", got or err)
    tool("pc_set_clipboard", {"text": prev})

    # 7. files
    f = os.path.join(WORK, "hello.txt")
    out, err = tool("pc_write_file", {"path": f, "content": "hello pcrc\n"})
    check("write_file", out and not out["isError"], err)
    out, err = tool("pc_read_file", {"path": f})
    check("read_file", out and out["parsed"].get("content") == "hello pcrc\n", out["parsed"] if out else err)
    out, err = tool("pc_list_directory", {"path": WORK})
    check("list_directory", out and any(e.get("name") == "hello.txt" for e in out["parsed"].get("entries", [])), err)
    out, err = tool("pc_copy_path", {"from": f, "to": os.path.join(WORK, "copy.txt")})
    check("copy_path", out and not out["isError"], err)
    out, err = tool("pc_move_path", {"from": os.path.join(WORK, "copy.txt"), "to": os.path.join(WORK, "moved.txt")})
    check("move_path", out and not out["isError"], err)
    out, err = tool("pc_file_info", {"path": os.path.join(WORK, "moved.txt")})
    check("file_info", out and out["parsed"].get("type") == "file", err)
    out, err = tool("pc_delete_path", {"path": os.path.join(WORK, "moved.txt")})
    check("delete_path", out and not out["isError"], err)

    # 8. shell
    out, err = tool("pc_shell_exec", {"command": "echo hello && pwd"})
    check("shell_exec", out and out["parsed"].get("exit_code") == 0 and "hello" in out["parsed"].get("stdout", ""), err)
    out, err = tool("pc_shell_exec", {"command": "sleep 2", "timeout_ms": 400})
    check("shell_exec timeout", out and out["parsed"].get("timed_out") is True, out["parsed"] if out else err)

    # 9. processes
    out, err = tool("pc_list_processes", {"limit": 5})
    check("list_processes", out and len(out["parsed"].get("processes", [])) <= 5, err)
    out, err = tool("pc_kill_process", {"pid": 999999})
    check("kill_process missing -> error", out and out["isError"], "")

    # 10. fetch_page (local page)
    out, err = tool("pc_fetch_page", {"url": BASE + "/"})
    check("fetch_page", out and not out["isError"] and "PC Remote Control MCP" in out["parsed"].get("content", ""), err)

    # 11. browser automation (headless)
    out, err = tool("pc_browser_open", {"url": BASE + "/", "headless": True, "wait_ms": 15000})
    bopen = out["parsed"] if out and isinstance(out["parsed"], dict) else {}
    check("browser_open", out and not out["isError"] and bopen.get("title") is not None, (bopen or err))
    out, err = tool("pc_browser_get_content", {"format": "text"})
    check("browser_get_content", out and "PC Remote Control MCP" in out["parsed"].get("content", ""), err)
    out, err = tool("pc_browser_eval", {"expression": "1+2"})
    check("browser_eval", out and out["parsed"].get("result") == "3", out["parsed"] if out else err)
    out, err = tool("pc_browser_navigate", {"url": "data:text/html,<title>T</title><h1>Hello</h1><button id='b' onclick=\"document.title='clicked'\">Go</button><input id='i'>"})
    check("browser_navigate data url", out and not out["isError"], err)
    out, err = tool("pc_browser_click", {"selector": "#b"})
    check("browser_click selector", out and not out["isError"], err)
    out, err = tool("pc_browser_eval", {"expression": "document.title"})
    check("browser click took effect", out and out["parsed"].get("result") == "clicked", out["parsed"] if out else err)
    out, err = tool("pc_browser_type", {"selector": "#i", "text": "hello browser"})
    out2, _ = tool("pc_browser_eval", {"expression": "document.querySelector('#i').value"})
    check("browser_type", out and not out["isError"] and out2 and out2["parsed"].get("result") == "hello browser",
          out2["parsed"] if out2 else err)
    out, err = tool("pc_browser_screenshot", {})
    ok = out and not out["isError"] and any(c.get("type") == "image" for c in out["raw"]["content"])
    check("browser_screenshot", ok, err)
    out, err = tool("pc_browser_tabs", {"action": "list"})
    check("browser_tabs list", out and out["parsed"].get("count", 0) >= 1, err)
    out, err = tool("pc_browser_close", {})
    check("browser_close", out and not out["isError"], err)

    # 12. documents
    try:
        import docx
        d = docx.Document()
        d.add_heading("PCRC Test", 0)
        d.add_paragraph("Hello from docx.")
        t = d.add_table(rows=2, cols=2)
        t.cell(0, 0).text = "cell-a"
        t.cell(0, 1).text = "cell-b"
        d.save(os.path.join(WORK, "test.docx"))
        docx_ok = True
    except Exception as e:
        docx_ok = False
        print("docx generation failed:", e)
    try:
        import openpyxl
        wb = openpyxl.Workbook()
        ws = wb.active
        ws.title = "Data"
        ws.append(["name", "value"])
        ws.append(["alpha", 1])
        ws.append(["beta", 2])
        wb.save(os.path.join(WORK, "test.xlsx"))
        xlsx_ok = True
    except Exception as e:
        xlsx_ok = False
        print("xlsx generation failed:", e)
    open(os.path.join(WORK, "test.md"), "w").write("# Title\n\n- one\n- two\n")
    open(os.path.join(WORK, "test.html"), "w").write("<html><body><h1>Head</h1><p>Para <b>bold</b></p><script>x=1</script></body></html>")

    if docx_ok:
        out, err = tool("pc_doc_read", {"path": os.path.join(WORK, "test.docx")})
        text = out["parsed"].get("content", "") if out and isinstance(out["parsed"], dict) else ""
        check("doc_read docx", out and not out["isError"] and "Hello from docx." in text and "cell-a" in text,
              text[:200] or err)
        out, err = tool("pc_doc_info", {"path": os.path.join(WORK, "test.docx")})
        check("doc_info docx", out and out["parsed"].get("kind") == "docx", err)
        out, err = tool("pc_doc_convert", {"input": os.path.join(WORK, "test.docx"),
                                            "output": os.path.join(WORK, "test.pdf"), "format": "pdf"})
        check("doc_convert docx->pdf", out and not out["isError"], (out["parsed"] if out else err))
        if out and not out["isError"]:
            out, err = tool("pc_doc_read", {"path": os.path.join(WORK, "test.pdf")})
            text = out["parsed"].get("content", "") if out and isinstance(out["parsed"], dict) else ""
            check("doc_read pdf", out and not out["isError"] and "Hello from docx" in text, text[:200] or err)
            out, err = tool("pc_doc_info", {"path": os.path.join(WORK, "test.pdf")})
            check("doc_info pdf pages", out and out["parsed"].get("pages") == 1, err)
    if xlsx_ok:
        out, err = tool("pc_doc_read", {"path": os.path.join(WORK, "test.xlsx")})
        text = out["parsed"].get("content", "") if out and isinstance(out["parsed"], dict) else ""
        check("doc_read xlsx", out and not out["isError"] and "alpha" in text and "beta" in text, text[:200] or err)
    out, err = tool("pc_doc_read", {"path": os.path.join(WORK, "test.md")})
    check("doc_read markdown", out and "# Title" in (out["parsed"].get("content", "") if out else ""), err)
    out, err = tool("pc_doc_read", {"path": os.path.join(WORK, "test.html")})
    check("doc_read html", out and "Head" in (out["parsed"].get("content", "") if out else "")
          and "x=1" not in (out["parsed"].get("content", "") if out else ""), err)
    out, err = tool("pc_doc_ocr", {"path": os.path.join(WORK, "test.md")})
    check("doc_ocr wrong type -> error", out and out["isError"], "")

    # 13. agents
    out, err = tool("pc_agent_list")
    agents = out["parsed"].get("agents", []) if out and isinstance(out["parsed"], dict) else []
    installed = [a["name"] for a in agents if a.get("installed")]
    check("agent_list detects CLIs", out and not out["isError"] and agents, installed)
    print("      installed agents:", installed)
    out, err = tool("pc_agent_run", {"agent": "custom", "command": "python3",
                                      "args": ["-c", "print('agent ok')"], "prompt": "ignored",
                                      "cwd": WORK, "wait_ms": 15000})
    check("agent_run custom", out and not out["isError"] and "agent ok" in out["parsed"].get("stdout", ""),
          (out["parsed"] if out else err))
    out, err = tool("pc_agent_start", {"agent": "custom", "command": "sh", "args": ["-c", "sleep 30"],
                                        "prompt": "x", "cwd": WORK})
    job = out["parsed"].get("job_id") if out and isinstance(out["parsed"], dict) else None
    check("agent_start job", bool(job), err)
    if job:
        out, err = tool("pc_agent_status", {"job_id": job})
        check("agent_status running", out and out["parsed"].get("status") == "running", err)
        out, err = tool("pc_agent_stop", {"job_id": job})
        check("agent_stop", out and out["parsed"].get("status") in ("stopped", "failed"), out["parsed"] if out else err)
    out, err = tool("pc_agent_jobs")
    check("agent_jobs list", out and out["parsed"].get("count", 0) >= 1, err)

    # 14. admin/service (read-only)
    out, err = tool("pc_service", {"action": "status", "name": "dbus"})
    check("service status dbus", out and not out["isError"], (out["parsed"] if out else err))
    out, err = tool("pc_admin_exec", {"command": "true"})
    # Either succeeds (passwordless sudo/root) or fails with the guidance error.
    ok = out is not None and (not out["isError"] or "sudo" in json.dumps(out["parsed"]))
    check("admin_exec true (safe)", ok, out["parsed"] if out else err)
    out, err = tool("pc_power", {"action": "nonsense"})
    check("power invalid action -> error", out and out["isError"], "")

    # 15. admin API + token gate
    st = admin("/admin/status")
    check("admin status", st.get("ok") is True and st.get("tool_prefix") == "pc" and st.get("tools", {}).get("total") == 54,
          {k: st.get(k) for k in ("bind", "port", "tool_prefix", "approval")})
    check("admin capabilities", isinstance(st.get("capabilities"), dict)
          and st["capabilities"].get("browser", {}).get("installed") is True, st.get("capabilities", {}).get("browser"))
    logs = admin("/admin/logs?limit=10")
    check("admin logs", isinstance(logs.get("entries"), list) and len(logs["entries"]) > 0, len(logs.get("entries", [])))
    cfg = admin("/admin/config")["config"]
    token = cfg["token"]
    token_cfg = dict(cfg)
    token_cfg["token_enabled"] = True
    admin_put(token_cfg)
    status, body = rpc("initialize", {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "t", "version": "1"}})
    check("token gate 401 without token", status == 401, f"status={status}")
    status, body = post_mcp({"jsonrpc": "2.0", "id": 1, "method": "initialize",
                             "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "t", "version": "1"}}}, token)
    check("token gate accepts token", status == 200 and body.get("result", {}).get("protocolVersion"), f"status={status}")
    admin_put(cfg)  # restore always-approve
    status, body = rpc("initialize", {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "t", "version": "1"}})
    check("always-approve restored", status == 200, f"status={status}")

    # 16. stdio transport
    init_line = json.dumps({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "stdio-test", "version": "1"}}})
    list_line = json.dumps({"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}})
    try:
        proc = subprocess.run([BIN, "--stdio"], input=(init_line + "\n" + list_line + "\n"),
                              capture_output=True, text=True, timeout=30)
        lines = [l for l in proc.stdout.splitlines() if l.strip()]
        ok = len(lines) >= 2 and json.loads(lines[0]).get("result", {}).get("serverInfo", {}).get("name") == "pc-remote-control-mcp"
        check("stdio transport", ok, proc.stderr[-200:] if not ok else f"{len(lines)} replies")
    except Exception as e:
        check("stdio transport", False, e)

    print()
    failed = [r for r in results if not r[1]]
    print(f"== {len(results) - len(failed)}/{len(results)} passed ==")
    for name, ok, detail in failed:
        print("FAILED:", name, "::", str(detail)[:300])
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
