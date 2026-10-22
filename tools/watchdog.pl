#!/usr/bin/perl
# 超时看门狗 —— 给「会一直跑下去的命令」套一个墙钟上限。
#
# 用法: watchdog.pl <秒> <命令...>
#
# ⚠️ **为什么不用 `perl -e 'alarm N; exec(@ARGV)'`**：POSIX 规定 `exec`
# 会**重置 alarm 定时器** ⇒ 那个写法**根本不会超时**（本仓实测：测试
# 二进制一直烧 CPU 十分钟不被杀）。这个版本改成「父进程不 exec、只
# fork」：父进程留着 alarm，超时后按**进程组** SIGKILL（被测命令与它
# spawn 出来的子进程**一起**被杀 ⇒ 不留孤儿进程）。
#
# 退出码: 0 = 正常；124 = **HANG**（超时被杀）；其它 = 被测命令的退出码。
#
# ⚠️ **macOS 没有 `timeout` 命令** ⇒ 这是本仓唯一可移植的写法。
use strict;
use warnings;

my $secs = shift @ARGV;
my @cmd  = @ARGV;
die "usage: watchdog.pl <secs> <cmd...>\n" unless defined $secs && @cmd;

my $pid = fork();
die "fork failed: $!\n" unless defined $pid;

if ($pid == 0) {
    setpgrp(0, 0);                      # 自成一个进程组 ⇒ 父进程能整组杀
    exec { $cmd[0] } @cmd;
    die "exec failed: $!\n";
}

my $timed_out = 0;
$SIG{ALRM} = sub {
    $timed_out = 1;
    kill 'KILL', -$pid if kill(0, -$pid);
};
alarm $secs;
waitpid($pid, 0);
my $rc = $?;
alarm 0;

if ($timed_out) {
    print STDERR "watchdog: TIMEOUT after $secs s => HANG（进程组已被 SIGKILL）\n";
    exit 124;
}
exit($rc >> 8);
