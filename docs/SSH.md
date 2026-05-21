# SSH 工作区配置指南

> 把远端服务器上的 Claude Code / Codex CLI 日志拉到本机一起生成周报。
> 跨平台说明（Windows / macOS / Linux），按本指南一次配置好，应用即可免交互同步。

---

## 1. 适用场景

- 工作机和开发服务器分开（如本地 Mac + 远程 Linux dev box）
- 把多台机器上的 AI 编程日志聚合到一份周报里
- 远端服务器跑 Claude Code 或 Codex CLI 留下了 `~/.claude/` / `~/.codex/` 日志

如果你只在一台机器上写代码，跳过本文，用应用启动时默认创建的「本机」工作区即可。

---

## 2. 前提条件

| 项 | 本机 | 远端 |
| --- | --- | --- |
| OpenSSH 客户端 | ✅ macOS / Linux 自带；Windows 10 1809+ 自带 | — |
| OpenSSH 服务端 | — | ✅ `sshd` 监听 22 或自定义端口 |
| `tar` 命令 | ✅ macOS / Linux / Windows 10 1803+ 自带 | ✅ 所有 Linux / macOS 默认装 |
| 网络可达 | ✅ 能直连远端（同网 / VPN / 公网） | — |

应用同步走 `ssh ... 'tar c' | tar x` 单向流（[ADR-013](./DECISIONS.md#adr-013放弃-rsync改用-ssh--tar-单向流)），不依赖 rsync，**Windows 用户不再需要单独装 rsync**。

---

## 3. 推荐：公钥免密

公钥认证一次配好后，应用每次同步都不需要密码输入，最稳。

### 3.1 生成密钥对

Windows（PowerShell）/ macOS / Linux 命令一致：

```bash
ssh-keygen -t ed25519 -C "weeklyreport@$(hostname)"
```

参数说明：
- `-t ed25519`：椭圆曲线密钥，比 RSA 短、快、安全，所有现代 OpenSSH 都支持
- `-C ...`：注释字段，方便在远端 `authorized_keys` 里识别这把钥匙是哪台机生成的

交互提示：
- **路径**：回车用默认（`~/.ssh/id_ed25519`）。如果已有同名密钥不要覆盖，按 `n` 退出，复用现有那把
- **passphrase**：留空直接回车——应用就能完全免交互同步。填了的话需要再配 `ssh-agent`，复杂度上升

生成完后会有两个文件：

| 文件 | 性质 | 处理 |
| --- | --- | --- |
| `~/.ssh/id_ed25519` | **私钥** | **永远不要分享**，权限自动 `600` |
| `~/.ssh/id_ed25519.pub` | 公钥 | 公开无所谓，要传到远端 |

### 3.2 推送公钥到远端

#### macOS / Linux

一条命令，会要求输入一次远端密码：

```bash
ssh-copy-id user@host
```

#### Windows（PowerShell）

PowerShell 没有 `ssh-copy-id`，手动拼一条等价命令：

```powershell
Get-Content $env:USERPROFILE\.ssh\id_ed25519.pub `
  | ssh user@host "mkdir -p ~/.ssh && chmod 700 ~/.ssh && cat >> ~/.ssh/authorized_keys && chmod 600 ~/.ssh/authorized_keys"
```

会要求输入一次远端密码。`>> authorized_keys` 是追加而非覆盖，远端已有的其他公钥不会被清掉。

把 `user` 和 `host` 替换成你的远端用户名和地址（如 `changan@dev.example.com`）。

### 3.3 验证免密登录

```bash
ssh -o BatchMode=yes user@host 'echo OK && tar --version | head -1'
```

正常会看到：

```
OK
tar (GNU tar) 1.35
```

如果仍要输入密码或报 `Permission denied (publickey)`：
- 检查远端 `~/.ssh/` 是 `700`、`authorized_keys` 是 `600`（权限太宽 sshd 会拒绝）
- 检查远端 `cat ~/.ssh/authorized_keys` 末尾确实有你这台机的公钥
- 远端 sshd 是否禁了公钥认证（极罕见，检查 `/etc/ssh/sshd_config` 的 `PubkeyAuthentication yes`）

---

## 4. 备选：密码认证

仅在远端禁用了公钥、或你不希望分发公钥时使用。注意：

- **Windows 不推荐**：依赖 `sshpass`，PowerShell / cmd 默认没有；建议改用公钥（§3）
- **macOS / Linux**：需要先装 `sshpass`
  - Debian/Ubuntu：`sudo apt install sshpass`
  - macOS：`brew install hudochenkov/sshpass/sshpass`
  - CentOS/RHEL：`sudo yum install sshpass`

密码会以**明文**保存在应用配置里（`workspaces.json`），与 LLM API key 一致（[ADR-008](./DECISIONS.md#adr-008api-key-明文存储v010))，v0.2 计划改为系统 keyring。

---

## 5. 在应用里添加 SSH 工作区

打开 WeeklyReport →「工作区」页 →「添加工作区」→ 选「SSH 远程」，填：

| 字段 | 填什么 |
| --- | --- |
| Name | 任意标识名（如 `dev-server-01`） |
| Host | 服务器 IP 或域名（如 `dev.example.com`） |
| User | 登录用户名 |
| Port | 不填默认 22；非默认端口才填 |
| Auth method | `key`（默认） |
| SSH key | 留空使用 `~/.ssh/id_ed25519`，自定义路径才填 |
| SSH password | Auth method 选 `password` 时才填 |
| Claude path | `~/.claude`（默认）或自定义 |
| Codex path | `~/.codex`（默认）或自定义 |
| Tools | 勾选远端上有的（claude-code / codex） |

填完点「测试连接」，应该所有项 ✓。然后保存。

之后生成周报选这个工作区即可，应用会自动 `ssh + tar` 拉日志。

---

## 6. 常见问题

### Q1：测试连接报 `Permission denied (publickey)`
公钥认证失败。按 §3.3 末尾的检查清单排。

### Q2：测试连接报 `Connection timed out`
网络不通。检查防火墙是否放行端口、sshd 是否在跑、VPN 是否连接。

### Q3：测试连接报 `Host key verification failed`
应用代码已加 `StrictHostKeyChecking=no` 自动接受新 host，这个报错只会出现在你手动用系统 ssh 时（且没加这个选项）。手动测试时加上 `-o StrictHostKeyChecking=no` 即可。

### Q4：远端没有 Claude Code / Codex CLI 怎么办？
Tools 字段不勾选对应那个即可。应用只同步你勾选的工具的日志，不勾就跳过。

### Q5：远端 tar 命令缺失？
极罕见，所有主流 Linux / macOS 默认都装。`apt install tar` / `yum install tar` 一句话装上。

### Q6：私钥换地方了 / 忘了密钥路径
工作区编辑里 `SSH key` 字段填新路径即可。或者把新私钥放回 `~/.ssh/id_ed25519` 让应用走默认。

### Q7：能用一份密钥连多台远端吗？
能。把同一份 `id_ed25519.pub` 按 §3.2 推到每台远端即可，每个工作区都不用单独填 SSH key 路径。

---

更多技术细节见：
- [DECISIONS.md ADR-010](./DECISIONS.md#adr-010ssh-使用系统命令而非-ssh2-crate)：为什么用系统命令而不是 Rust SSH crate
- [DECISIONS.md ADR-012](./DECISIONS.md#adr-012ssh-密码认证通过-sshpass密码经-sshpass-环境变量传入)：密码认证的实现细节
- [DECISIONS.md ADR-013](./DECISIONS.md#adr-013放弃-rsync改用-ssh--tar-单向流)：为什么用 tar 而不是 rsync
