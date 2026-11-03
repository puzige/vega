# macOS 收尾机制限时验证记录

## 1. 授权、范围与时间盒

用户本轮确认“限时的平台安全验证研究阶段，先验证收尾机制，再继续补齐实现”，并在执行方式选择中明确批准：**本机隔离探针**；本轮一次性研究例外，系统 Python/ctypes 与 sandbox-exec，最多 10 分钟，只用自建临时目录与少量自行短时退出的子进程，不编译、不改 CI、不提权、不安装服务。该例外不延伸到 Vega 本地运行/测试或后续研究。

- 总研究时间盒：30 分钟，首次明确记录时间 `2026-11-03T07:30:07Z`，截止 `08:00:07Z`；其中包含等待用户确认执行环境，授权后记录时间为 `07:46:57Z`。
- 实际平台探针在 `07:50:33Z` 前结束；两次批次测量耗时分别为 0.606 秒、0.138 秒，另有一次无脱组行为的初始化预检。不是产品性能 benchmark。
- 仓库基线：`b2a3b2c11b722cf290a9bc8f242dace0b6537123`；实施检查点仍为 `5f2059be09ffe5a1d1df31253b9bb9f158806295`。修改前 fetch + ff-only：Already up to date，原工作树干净。
- 环境：macOS 15.8.2 / build 24H232 / arm64，Homebrew Python 3.14.7，Apple `/usr/bin/sandbox-exec`；普通当前用户，无新增 entitlement/签名/权限。
- 本阶段只形成研究文档；不改冻结[设计 §5.3](vega-long-command-waiting-design.md#53-停止与未知结果)、生产代码、CI 或支持命令合同。

## 2. 先验验收与执行顺序

研究目标不是“尽可能杀进程”，而是找到能保持终止边界、仍兼容普通工具链的实际部署候选。

| 用例 | 必须观察的事实 | 层级与判定 |
|---|---|---|
| 阳性对照 | 子进程非组长时直接 setsid/setpgid 可脱组；普通 spawn 继承组；两种 spawn 属性可改组/会话 | 独立真实平台探针；先验证观测有效 |
| 直接 syscall 过滤 | 直接调用受阻，同时 spawn 属性不能绕过 | 有任一路径脱组即淘汰，不能以直接调用被拒认定通过 |
| setcontrol 拒绝 | 直接调用与 spawn 属性均受约束 | 不能按规则名推测含义 |
| 全禁 spawn 对照 | 脱组受阻的同时，普通 spawn 应仍可用 | 普通 spawn 也被拒即不满足现有兼容合同；不擅自改产品验收 |
| 启用门槛 | 上述窄门槛通过后，才进入多代/竞态/取消/超时/关闭/能力撤销矩阵 | 窄门槛已失败，不再扩大实验，不启用会话 |

执行顺序：只读核对 SDK/Apple 固定源码与本机策略 → 无脱组初始化预检 → 4 种策略 × 5 条路径 → 诊断失败的观测入口 → 仅修正该入口补 5 项 → 回收检查并停止实验。

## 3. 可复核结果：最小候选未通过

基础策略为 `(version 1) (allow default)`，额外拒绝临时目录外的 `file-write*`；不是 Vega 生产 deny-default profile 的端到端验证。每次 worker 独立 session，直接调用在 fork 子进程执行；spawn 使用 SDK 公开 flags `0x0400` / `0x0002`。worker alarm 4 秒，spawn 子进程 alarm 2 秒并立即报告/退出，父进程逐一 waitpid。每条外层调用 timeout 7 秒，未触发超时。未做延迟写入、网络访问或真实 kill-tree 演示。

| 策略 | 直接 setsid | 直接 setpgid | 普通 spawn | spawn SETSID | spawn SETPGROUP |
|---|---|---|---|---|---|
| 基础阳性对照 | 成功离组 | 成功离组 | rc=0，继承原组 | rc=0，新 SID/PGID | rc=0，新 PGID |
| deny SYS_setsid / SYS_setpgid | EPERM (1) | EPERM (1) | rc=0，继承原组 | **仍成功离组** | **仍成功离组** |
| deny process-info-setcontrol | **仍成功离组** | **仍成功离组** | rc=0，继承原组 | **仍成功离组** | **仍成功离组** |
| 再 deny SYS_posix_spawn（修正入口后） | EPERM (1) | EPERM (1) | **rc=1，普通 spawn 也被拒** | rc=1 | rc=1 |

关键原始观察：

- deny_direct / spawn_setsid：parent PGID=19667；spawn rc=0；child PID=PGID=SID=19668，wait status=0。
- deny_direct / spawn_setpgroup：parent PGID=19669；spawn rc=0；child PID=PGID=19670、SID=19669，wait status=0。
- deny_setcontrol / setsid：parent PGID=19671；child PID=PGID=SID=19672，wait status=0。
- 修正后的全禁 spawn / spawn_plain：worker 已开始并输出 parent PID/PGID=20722；spawn rc=1，无 child 创建。
- 各有效子进程均已 waitpid，status=0；结束后对已记录的 worker/child 精确 PID 做只读 ps 查询，无存活匹配。不是全系统扫描或 kill 验证。

**结论：本机已取得反例，不再只是“尚无源码证明”。仅过滤直接 syscall，或去掉 process-info-setcontrol，都不保持进程组不变性；一并禁止整个 posix_spawn 的窄候选破坏普通工具链。没有获得符合冻结安全与兼容合同的方案，§5.3 启用前提未通过。**

这不证明所有 macOS 机制都不可能实现，不覆盖其他 OS/版本/签名，不证明拒绝所有 spawn 在其他路径下就已安全，也不等于存在经过验证的选择性 spawn 属性过滤器。

## 4. 首次失败与责任层修正

首批 20 项中，全禁 spawn 的 5 项在 Homebrew Python 启动器内提前失败：outer rc=1，stderr 为 `python3.14: posix_spawn: …/Resources/Python.app/Contents/MacOS/Python: Undefined error: 0`；无 worker 输出。**这 5 项没有进入被测调用，不能计为直接/spawn 路径验证通过。**

原因分类：观测入口/测试 harness 限制，同时也是全禁 spawn 破坏真实启动器的兼容性证据。先核对 sys.prefix 下真实 Python 二进制存在，再只将外层入口改为该二进制，保持策略、worker 与断言不变，补执行 5 项。原始首批记录保留，不覆盖、不通过重试换绿；修正后普通 spawn 仍明确被拒。

规则带 `(with send-signal SIGSYS)`，但本机实际直接调用结果是 errno=1，不按修饰符名字声称进程被 SIGSYS 杀死。

## 5. 静态证据与范围限制

- 本机 `/System/Library/Sandbox/Profiles/system.sb:175–176` 使用 `syscall-unix` / `syscall-number`；`com.apple.sharingd.sb:416` 列出 `SYS_posix_spawn`。system.sb SHA-256：`16c5f04132da6d244dd8496b3f4d27190df5bb70135d1b0f1f6789e0bf520912`。只说明存在策略词汇，不证明对普通第三方应用的长期支持合同。
- MacOSX15.5.sdk `usr/include/sys/spawn.h:46,61` 给出 SETPGROUP/SETSID；`spawn.h` 给出公开 attr 函数签名；不运行编译。
- [Apple 固定 XNU kern_exec.c:4485–4503](https://github.com/apple-oss-distributions/xnu/blob/f6217f891ac0bb64f3d375211650a4c1ff8ca1ea/bsd/kern/kern_exec.c#L4485-L4503) 在 spawn 内部调用 setpgid/setsid_internal。已读取完整文件，SHA-256：`89531d33618b8361064236189212c3c96be1b32e098690b17802b6b15d932d1a`。该源码不是本机内核身份，动态观察与其分别记录。
- 后续固定源码补抓遇 HTTP IncompleteRead（4134/17115 bytes）；未用该次不完整响应作新结论，没有自动重试。既有源码与实际探针已足以淘汰本轮候选。
- 本机系统策略范围搜索未找到 setsid/setpgid 或选择性 spawn flags 规则，**搜索未命中不是不存在证明**；第三方文章/Chromium 策略只用于定位 syscall 过滤方向，不作为 Apple 部署保证。

**未执行**：Vega 本地运行/编译/业务测试、云端 CI、真正进程树强杀、应用停止/关闭、fork 风暴、多代竞态、FD/mmap/网络撤销、跨版本/真实 App 签名验收、性能 benchmark。原验收矩阵不能标通过。

## 6. 决策、恢复条件与产物

本轮研究结束，候选被反例淘汰；**停止受阻的产品实现步骤，不继续补齐 UI/路由来制造已交付印象，不合并/发布检查点。** Issue 保持 OPEN / In progress，旧正文“设计未获确认”不再代表当前授权。

下一步需要的是具体产品/平台路径裁决，不是再次确认同一个研究阶段：

1. 若坚持当前任意 shell、普通应用部署与兼容合同：须先取得新的可部署选择性约束机制及其版本/权限合同（例如平台厂商可验证的接口说明），再提出针对该机制的验证；在此之前暂缓会话启用，保留原阻塞 bash。
2. 若愿意改用更强隔离域（如 VM）或接受受限命令/运行环境：这是新设计边界，须另行明确授权、成本与验收；本轮没有采用或实现这些方案。

安全前提成立后，才从[原检查点](implementation-checkpoint.md)继续补集成/回归、按既有 Actions 验证、非实现者审查及最终 head 门禁；不重建已有工作，不恢复未授权 E2E。

原始临时证据目录为 `/private/tmp/vega-412-platform.ZU0CzS`（保留，未删除）。以下源码和 JSON 以 Markdown 附录固定，**不是新增可执行仓库测试/CI 接线**；包含首次无效批次，便于后续审计，不需要为结论再运行。

| 文件 | SHA-256 |
|---|---|
| probe.py | `8f4d79eec93bee898d55899d22e27cd9f10db9a4cc681f6cf4bab88788945b72` |
| spawn_control.py | `aa5a37aba1332844568a02285b0f08886714609ae19c4596de784587e1788437` |
| results.json | `8996a13d7cc2930d2f638a8d606dd95b1778a72edf5db245dd0fa86a9f1b53f2` |
| spawn-control-results.json | `d2ea71f644e2395b5376788df0f1ebab6b79655097f4b8dec1c0fd21b0114a29` |

文档验证与版本由 Issue 最终交付记录回写；本文不预先宣称审查/CI/合并通过。

## 附录：probe.py

```python
import ctypes
import hashlib
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parent
CHILD = "import os,signal,json; signal.alarm(2); print(json.dumps({'child_pid':os.getpid(),'child_pgid':os.getpgrp(),'child_sid':os.getsid(0)}),flush=True)"


def emit(value):
    print(json.dumps(value, sort_keys=True), flush=True)


def worker(mode):
    signal.alarm(4)
    emit({'parent_pid': os.getpid(), 'parent_pgid': os.getpgrp(), 'mode': mode})
    if mode in ('setsid', 'setpgid'):
        pid = os.fork()
        if pid == 0:
            try:
                if mode == 'setsid':
                    os.setsid()
                else:
                    os.setpgid(0, 0)
                emit({'direct': mode, 'result': 'changed', 'child_pid': os.getpid(), 'child_pgid': os.getpgrp(), 'child_sid': os.getsid(0)})
            except OSError as exc:
                emit({'direct': mode, 'errno': exc.errno})
            os._exit(0)
        _, status = os.waitpid(pid, 0)
        emit({'wait_status': status})
        return
    libc = ctypes.CDLL('/usr/lib/libSystem.B.dylib', use_errno=True)
    attr = ctypes.c_void_p()
    ap = ctypes.POINTER(ctypes.c_void_p)
    libc.posix_spawnattr_init.argtypes = [ap]
    libc.posix_spawnattr_setflags.argtypes = [ap, ctypes.c_short]
    libc.posix_spawnattr_setpgroup.argtypes = [ap, ctypes.c_int]
    libc.posix_spawnattr_destroy.argtypes = [ap]
    libc.posix_spawn.argtypes = [ctypes.POINTER(ctypes.c_int), ctypes.c_char_p, ctypes.c_void_p, ap, ctypes.POINTER(ctypes.c_char_p), ctypes.POINTER(ctypes.c_char_p)]
    funcs = ('posix_spawnattr_init', 'posix_spawnattr_setflags', 'posix_spawnattr_setpgroup', 'posix_spawnattr_destroy', 'posix_spawn')
    for name in funcs:
        getattr(libc, name).restype = ctypes.c_int
    rc = libc.posix_spawnattr_init(ctypes.byref(attr))
    if rc:
        raise RuntimeError(('attr_init', rc))
    try:
        flags = {'spawn_plain': 0, 'spawn_setsid': 0x0400, 'spawn_setpgroup': 0x0002}[mode]
        rc = libc.posix_spawnattr_setflags(ctypes.byref(attr), flags)
        if rc:
            raise RuntimeError(('setflags', rc))
        if mode == 'spawn_setpgroup':
            rc = libc.posix_spawnattr_setpgroup(ctypes.byref(attr), 0)
            if rc:
                raise RuntimeError(('setpgroup', rc))
        args = [os.fsencode(sys.executable), b'-I', b'-B', b'-c', CHILD.encode()]
        argv = (ctypes.c_char_p * (len(args) + 1))(*args, None)
        envp = (ctypes.c_char_p * 1)(None)
        pid = ctypes.c_int()
        rc = libc.posix_spawn(ctypes.byref(pid), args[0], None, ctypes.byref(attr), argv, envp)
        emit({'spawn_rc': rc})
        if rc == 0:
            _, status = os.waitpid(pid.value, 0)
            emit({'wait_status': status})
    finally:
        libc.posix_spawnattr_destroy(ctypes.byref(attr))


def main():
    profiles = {
        'control': '',
        'deny_direct': '(deny syscall-unix (syscall-number SYS_setsid SYS_setpgid) (with send-signal SIGSYS))',
        'deny_setcontrol': '(deny process-info-setcontrol)',
        'deny_spawn_and_direct': '(deny syscall-unix (syscall-number SYS_setsid SYS_setpgid SYS_posix_spawn) (with send-signal SIGSYS))',
    }
    results = []
    started = time.monotonic()
    for name, rule in profiles.items():
        profile = '(version 1) (allow default) (deny file-write* (require-not (subpath "' + str(ROOT) + '"))) ' + rule
        for mode in ('setsid', 'setpgid', 'spawn_plain', 'spawn_setsid', 'spawn_setpgroup'):
            cmd = ['/usr/bin/sandbox-exec', '-p', profile, sys.executable, '-I', '-B', str(Path(__file__).resolve()), '--worker', mode]
            run = subprocess.run(cmd, capture_output=True, text=True, timeout=7, start_new_session=True, cwd=ROOT)
            result = {'profile': name, 'rule': rule, 'mode': mode, 'returncode': run.returncode, 'stdout': run.stdout, 'stderr': run.stderr}
            results.append(result)
            emit(result)
    summary = {'duration_seconds': round(time.monotonic() - started, 3), 'python': sys.executable, 'source_sha256': hashlib.sha256(Path(__file__).read_bytes()).hexdigest(), 'results': results}
    (ROOT / 'results.json').write_text(json.dumps(summary, indent=2) + '\n')
    emit({'duration_seconds': summary['duration_seconds'], 'source_sha256': summary['source_sha256'], 'cases': len(results)})


if __name__ == '__main__':
    if len(sys.argv) == 3 and sys.argv[1] == '--worker':
        worker(sys.argv[2])
    else:
        main()
```

## 附录：spawn_control.py

```python
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parent
PYTHON = (Path(sys.prefix) / 'Resources/Python.app/Contents/MacOS/Python').resolve()
RULE = '(deny syscall-unix (syscall-number SYS_setsid SYS_setpgid SYS_posix_spawn) (with send-signal SIGSYS))'
PROFILE = '(version 1) (allow default) (deny file-write* (require-not (subpath "' + str(ROOT) + '"))) ' + RULE
results = []
started = time.monotonic()
for mode in ('setsid', 'setpgid', 'spawn_plain', 'spawn_setsid', 'spawn_setpgroup'):
    run = subprocess.run(['/usr/bin/sandbox-exec', '-p', PROFILE, str(PYTHON), '-I', '-B', str(ROOT / 'probe.py'), '--worker', mode], capture_output=True, text=True, timeout=7, start_new_session=True, cwd=ROOT)
    result = {'profile': 'deny_spawn_and_direct', 'mode': mode, 'returncode': run.returncode, 'stdout': run.stdout, 'stderr': run.stderr}
    results.append(result)
    print(json.dumps(result, sort_keys=True), flush=True)
summary = {'duration_seconds': round(time.monotonic() - started, 3), 'python': str(PYTHON), 'source_sha256': hashlib.sha256(Path(__file__).read_bytes()).hexdigest(), 'worker_sha256': hashlib.sha256((ROOT / 'probe.py').read_bytes()).hexdigest(), 'results': results}
(ROOT / 'spawn-control-results.json').write_text(json.dumps(summary, indent=2) + '\n')
print(json.dumps({k: v for k, v in summary.items() if k != 'results'}, sort_keys=True))
```

## 附录：results.json

```json
{
  "duration_seconds": 0.606,
  "python": "/opt/homebrew/opt/python@3.14/bin/python3.14",
  "source_sha256": "8f4d79eec93bee898d55899d22e27cd9f10db9a4cc681f6cf4bab88788945b72",
  "results": [
    {
      "profile": "control",
      "rule": "",
      "mode": "setsid",
      "returncode": 0,
      "stdout": "{\"mode\": \"setsid\", \"parent_pgid\": 19650, \"parent_pid\": 19650}\n{\"child_pgid\": 19651, \"child_pid\": 19651, \"child_sid\": 19651, \"direct\": \"setsid\", \"result\": \"changed\"}\n{\"wait_status\": 0}\n",
      "stderr": ""
    },
    {
      "profile": "control",
      "rule": "",
      "mode": "setpgid",
      "returncode": 0,
      "stdout": "{\"mode\": \"setpgid\", \"parent_pgid\": 19653, \"parent_pid\": 19653}\n{\"child_pgid\": 19654, \"child_pid\": 19654, \"child_sid\": 19653, \"direct\": \"setpgid\", \"result\": \"changed\"}\n{\"wait_status\": 0}\n",
      "stderr": ""
    },
    {
      "profile": "control",
      "rule": "",
      "mode": "spawn_plain",
      "returncode": 0,
      "stdout": "{\"mode\": \"spawn_plain\", \"parent_pgid\": 19655, \"parent_pid\": 19655}\n{\"spawn_rc\": 0}\n{\"child_pid\": 19656, \"child_pgid\": 19655, \"child_sid\": 19655}\n{\"wait_status\": 0}\n",
      "stderr": ""
    },
    {
      "profile": "control",
      "rule": "",
      "mode": "spawn_setsid",
      "returncode": 0,
      "stdout": "{\"mode\": \"spawn_setsid\", \"parent_pgid\": 19657, \"parent_pid\": 19657}\n{\"spawn_rc\": 0}\n{\"child_pid\": 19658, \"child_pgid\": 19658, \"child_sid\": 19658}\n{\"wait_status\": 0}\n",
      "stderr": ""
    },
    {
      "profile": "control",
      "rule": "",
      "mode": "spawn_setpgroup",
      "returncode": 0,
      "stdout": "{\"mode\": \"spawn_setpgroup\", \"parent_pgid\": 19659, \"parent_pid\": 19659}\n{\"spawn_rc\": 0}\n{\"child_pid\": 19660, \"child_pgid\": 19660, \"child_sid\": 19659}\n{\"wait_status\": 0}\n",
      "stderr": ""
    },
    {
      "profile": "deny_direct",
      "rule": "(deny syscall-unix (syscall-number SYS_setsid SYS_setpgid) (with send-signal SIGSYS))",
      "mode": "setsid",
      "returncode": 0,
      "stdout": "{\"mode\": \"setsid\", \"parent_pgid\": 19661, \"parent_pid\": 19661}\n{\"direct\": \"setsid\", \"errno\": 1}\n{\"wait_status\": 0}\n",
      "stderr": ""
    },
    {
      "profile": "deny_direct",
      "rule": "(deny syscall-unix (syscall-number SYS_setsid SYS_setpgid) (with send-signal SIGSYS))",
      "mode": "setpgid",
      "returncode": 0,
      "stdout": "{\"mode\": \"setpgid\", \"parent_pgid\": 19663, \"parent_pid\": 19663}\n{\"direct\": \"setpgid\", \"errno\": 1}\n{\"wait_status\": 0}\n",
      "stderr": ""
    },
    {
      "profile": "deny_direct",
      "rule": "(deny syscall-unix (syscall-number SYS_setsid SYS_setpgid) (with send-signal SIGSYS))",
      "mode": "spawn_plain",
      "returncode": 0,
      "stdout": "{\"mode\": \"spawn_plain\", \"parent_pgid\": 19665, \"parent_pid\": 19665}\n{\"spawn_rc\": 0}\n{\"child_pid\": 19666, \"child_pgid\": 19665, \"child_sid\": 19665}\n{\"wait_status\": 0}\n",
      "stderr": ""
    },
    {
      "profile": "deny_direct",
      "rule": "(deny syscall-unix (syscall-number SYS_setsid SYS_setpgid) (with send-signal SIGSYS))",
      "mode": "spawn_setsid",
      "returncode": 0,
      "stdout": "{\"mode\": \"spawn_setsid\", \"parent_pgid\": 19667, \"parent_pid\": 19667}\n{\"spawn_rc\": 0}\n{\"child_pid\": 19668, \"child_pgid\": 19668, \"child_sid\": 19668}\n{\"wait_status\": 0}\n",
      "stderr": ""
    },
    {
      "profile": "deny_direct",
      "rule": "(deny syscall-unix (syscall-number SYS_setsid SYS_setpgid) (with send-signal SIGSYS))",
      "mode": "spawn_setpgroup",
      "returncode": 0,
      "stdout": "{\"mode\": \"spawn_setpgroup\", \"parent_pgid\": 19669, \"parent_pid\": 19669}\n{\"spawn_rc\": 0}\n{\"child_pid\": 19670, \"child_pgid\": 19670, \"child_sid\": 19669}\n{\"wait_status\": 0}\n",
      "stderr": ""
    },
    {
      "profile": "deny_setcontrol",
      "rule": "(deny process-info-setcontrol)",
      "mode": "setsid",
      "returncode": 0,
      "stdout": "{\"mode\": \"setsid\", \"parent_pgid\": 19671, \"parent_pid\": 19671}\n{\"child_pgid\": 19672, \"child_pid\": 19672, \"child_sid\": 19672, \"direct\": \"setsid\", \"result\": \"changed\"}\n{\"wait_status\": 0}\n",
      "stderr": ""
    },
    {
      "profile": "deny_setcontrol",
      "rule": "(deny process-info-setcontrol)",
      "mode": "setpgid",
      "returncode": 0,
      "stdout": "{\"mode\": \"setpgid\", \"parent_pgid\": 19673, \"parent_pid\": 19673}\n{\"child_pgid\": 19674, \"child_pid\": 19674, \"child_sid\": 19673, \"direct\": \"setpgid\", \"result\": \"changed\"}\n{\"wait_status\": 0}\n",
      "stderr": ""
    },
    {
      "profile": "deny_setcontrol",
      "rule": "(deny process-info-setcontrol)",
      "mode": "spawn_plain",
      "returncode": 0,
      "stdout": "{\"mode\": \"spawn_plain\", \"parent_pgid\": 19675, \"parent_pid\": 19675}\n{\"spawn_rc\": 0}\n{\"child_pid\": 19676, \"child_pgid\": 19675, \"child_sid\": 19675}\n{\"wait_status\": 0}\n",
      "stderr": ""
    },
    {
      "profile": "deny_setcontrol",
      "rule": "(deny process-info-setcontrol)",
      "mode": "spawn_setsid",
      "returncode": 0,
      "stdout": "{\"mode\": \"spawn_setsid\", \"parent_pgid\": 19677, \"parent_pid\": 19677}\n{\"spawn_rc\": 0}\n{\"child_pid\": 19678, \"child_pgid\": 19678, \"child_sid\": 19678}\n{\"wait_status\": 0}\n",
      "stderr": ""
    },
    {
      "profile": "deny_setcontrol",
      "rule": "(deny process-info-setcontrol)",
      "mode": "spawn_setpgroup",
      "returncode": 0,
      "stdout": "{\"mode\": \"spawn_setpgroup\", \"parent_pgid\": 19679, \"parent_pid\": 19679}\n{\"spawn_rc\": 0}\n{\"child_pid\": 19680, \"child_pgid\": 19680, \"child_sid\": 19679}\n{\"wait_status\": 0}\n",
      "stderr": ""
    },
    {
      "profile": "deny_spawn_and_direct",
      "rule": "(deny syscall-unix (syscall-number SYS_setsid SYS_setpgid SYS_posix_spawn) (with send-signal SIGSYS))",
      "mode": "setsid",
      "returncode": 1,
      "stdout": "",
      "stderr": "python3.14: posix_spawn: /opt/homebrew/Cellar/python@3.14/3.14.7/Frameworks/Python.framework/Versions/3.14/Resources/Python.app/Contents/MacOS/Python: Undefined error: 0\n"
    },
    {
      "profile": "deny_spawn_and_direct",
      "rule": "(deny syscall-unix (syscall-number SYS_setsid SYS_setpgid SYS_posix_spawn) (with send-signal SIGSYS))",
      "mode": "setpgid",
      "returncode": 1,
      "stdout": "",
      "stderr": "python3.14: posix_spawn: /opt/homebrew/Cellar/python@3.14/3.14.7/Frameworks/Python.framework/Versions/3.14/Resources/Python.app/Contents/MacOS/Python: Undefined error: 0\n"
    },
    {
      "profile": "deny_spawn_and_direct",
      "rule": "(deny syscall-unix (syscall-number SYS_setsid SYS_setpgid SYS_posix_spawn) (with send-signal SIGSYS))",
      "mode": "spawn_plain",
      "returncode": 1,
      "stdout": "",
      "stderr": "python3.14: posix_spawn: /opt/homebrew/Cellar/python@3.14/3.14.7/Frameworks/Python.framework/Versions/3.14/Resources/Python.app/Contents/MacOS/Python: Undefined error: 0\n"
    },
    {
      "profile": "deny_spawn_and_direct",
      "rule": "(deny syscall-unix (syscall-number SYS_setsid SYS_setpgid SYS_posix_spawn) (with send-signal SIGSYS))",
      "mode": "spawn_setsid",
      "returncode": 1,
      "stdout": "",
      "stderr": "python3.14: posix_spawn: /opt/homebrew/Cellar/python@3.14/3.14.7/Frameworks/Python.framework/Versions/3.14/Resources/Python.app/Contents/MacOS/Python: Undefined error: 0\n"
    },
    {
      "profile": "deny_spawn_and_direct",
      "rule": "(deny syscall-unix (syscall-number SYS_setsid SYS_setpgid SYS_posix_spawn) (with send-signal SIGSYS))",
      "mode": "spawn_setpgroup",
      "returncode": 1,
      "stdout": "",
      "stderr": "python3.14: posix_spawn: /opt/homebrew/Cellar/python@3.14/3.14.7/Frameworks/Python.framework/Versions/3.14/Resources/Python.app/Contents/MacOS/Python: Undefined error: 0\n"
    }
  ]
}
```

## 附录：spawn-control-results.json

```json
{
  "duration_seconds": 0.138,
  "python": "/opt/homebrew/Cellar/python@3.14/3.14.7/Frameworks/Python.framework/Versions/3.14/Resources/Python.app/Contents/MacOS/Python",
  "source_sha256": "aa5a37aba1332844568a02285b0f08886714609ae19c4596de784587e1788437",
  "worker_sha256": "8f4d79eec93bee898d55899d22e27cd9f10db9a4cc681f6cf4bab88788945b72",
  "results": [
    {
      "profile": "deny_spawn_and_direct",
      "mode": "setsid",
      "returncode": 0,
      "stdout": "{\"mode\": \"setsid\", \"parent_pgid\": 20718, \"parent_pid\": 20718}\n{\"direct\": \"setsid\", \"errno\": 1}\n{\"wait_status\": 0}\n",
      "stderr": ""
    },
    {
      "profile": "deny_spawn_and_direct",
      "mode": "setpgid",
      "returncode": 0,
      "stdout": "{\"mode\": \"setpgid\", \"parent_pgid\": 20720, \"parent_pid\": 20720}\n{\"direct\": \"setpgid\", \"errno\": 1}\n{\"wait_status\": 0}\n",
      "stderr": ""
    },
    {
      "profile": "deny_spawn_and_direct",
      "mode": "spawn_plain",
      "returncode": 0,
      "stdout": "{\"mode\": \"spawn_plain\", \"parent_pgid\": 20722, \"parent_pid\": 20722}\n{\"spawn_rc\": 1}\n",
      "stderr": ""
    },
    {
      "profile": "deny_spawn_and_direct",
      "mode": "spawn_setsid",
      "returncode": 0,
      "stdout": "{\"mode\": \"spawn_setsid\", \"parent_pgid\": 20723, \"parent_pid\": 20723}\n{\"spawn_rc\": 1}\n",
      "stderr": ""
    },
    {
      "profile": "deny_spawn_and_direct",
      "mode": "spawn_setpgroup",
      "returncode": 0,
      "stdout": "{\"mode\": \"spawn_setpgroup\", \"parent_pgid\": 20724, \"parent_pid\": 20724}\n{\"spawn_rc\": 1}\n",
      "stderr": ""
    }
  ]
}
```
