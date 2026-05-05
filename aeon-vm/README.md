# AEON Flow

> 程序可以在任意时刻暂停，迁移到另一台机器，在迁移过程中被检查和修改，然后继续执行。

## 安装

```bash
git clone <repo>
cd AEON-FLOW/aeon-vm
cargo build --release
# 把 target/release/ 加入 PATH
```

## 5 分钟上手

```bash
# 1. 写一个程序
cat programs/fibonacci.asm

# 2. 编译
aeon-asm programs/fibonacci.asm

# 3. 运行
aeon-run fibonacci.aeon
# r2 = 55

# 4. 暂停并迁移（两个终端）
aeon-recv --session alice@laptop/conv-1   # 终端 1
aeon-send fibonacci.aeon --snap-at 5 --to 127.0.0.1:9999  # 终端 2

# 5. 在控制台里修改后继续
aeon> set reg 0 3
aeon> resume
# 总步数 23，r2 = 3
```

## 两账户协作

```bash
# Alice
aeon> share collab-1

# Bob
aeon> join collab-1
aeon> history       # 看到 Alice 的操作
aeon> set reg 1 99
aeon> resume        # 两人改动合并生效
```

## 指令集

见 docs/ISA.md

## 已知限制

见 KNOWN_LIMITATIONS.md
