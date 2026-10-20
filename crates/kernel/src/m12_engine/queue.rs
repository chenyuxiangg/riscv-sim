//! M-12 事件队列 — 稳定排序的最小堆 (stable-ordered min-heap).
//!
//! 排序键 = `(at_ps, seq)`：先按虚拟时间升序，同一虚拟时间再按**插入序**
//! `seq` 升序（M-12 §3.2「稳定排序键：time 升序，同 time 按 seq 升序」）。
//! `seq` 由引擎的单一计数器分配，保证「同种子 + 同输入序列 → 结果唯一」
//! （FR-12 / AD §8.1 D0）。
//!
//! 队列快照以「规范出队序」的形式给出（[`EventQueue::to_ordered_vec`]）：
//! 快照与恢复都按该顺序重建，因此恢复后的出队序列与恢复前逐条一致
//! （D-05）。

use std::cmp::Ordering;
use std::collections::BinaryHeap;

use super::SchedEvent;

/// 一个排队中的事件：排序键 + 载荷。
///
/// **不变量**：同一个引擎的队列里不会出现两个 `(at_ps, seq)` 相同的条目
/// —— `seq` 由 [`crate::m12_engine::Engine`] 的单调计数器分配，恢复快照时
/// 也会校正计数器以避开已占用的 `seq`（D-17）。因此这里的 `Ord` 在活动
/// 队列上是一个全序。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueuedEvent {
    /// 事件到期时刻（虚拟时间，ps）。
    pub at_ps: u64,
    /// 插入序 —— 同到期时刻的稳定排序锚点，同时是取消令牌。
    pub seq: u64,
    /// 事件载荷。
    pub event: SchedEvent,
}

impl QueuedEvent {
    /// 规范排序键：`(到期时刻, 插入序)`，升序。
    pub fn sort_key(&self) -> (u64, u64) {
        (self.at_ps, self.seq)
    }
}

impl Ord for QueuedEvent {
    fn cmp(&self, other: &Self) -> Ordering {
        // 反向比较是刻意的：`BinaryHeap` 是最大堆，我们要把**最早**的
        // 事件放在堆顶；同一时刻按插入序定序，保证同刻事件的出队顺序
        // 与入队顺序一致（确定性）。
        other
            .at_ps
            .cmp(&self.at_ps)
            .then_with(|| other.seq.cmp(&self.seq))
    }
}

impl PartialOrd for QueuedEvent {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// 引擎的待处理事件队列。
///
/// 只暴露「入队 / 看顶 / 出队 / 快照」四类操作；排序规则完全由
/// [`QueuedEvent`] 的 `Ord` 决定，因此堆的内部布局不影响任何可观测行为。
#[derive(Debug, Clone, Default)]
pub struct EventQueue {
    heap: BinaryHeap<QueuedEvent>,
}

impl EventQueue {
    /// 空队列。
    pub fn new() -> Self {
        Self {
            heap: BinaryHeap::new(),
        }
    }

    /// 待处理事件数。
    pub fn len(&self) -> usize {
        self.heap.len()
    }

    /// 队列是否为空。
    pub fn is_empty(&self) -> bool {
        self.heap.is_empty()
    }

    /// 入队。
    pub fn push(&mut self, event: QueuedEvent) {
        self.heap.push(event);
    }

    /// 看堆顶（最早到期的事件），不弹出。
    pub fn peek(&self) -> Option<&QueuedEvent> {
        self.heap.peek()
    }

    /// 弹出堆顶（最早到期的事件）。
    pub fn pop(&mut self) -> Option<QueuedEvent> {
        self.heap.pop()
    }

    /// 按**规范出队序**导出待处理事件 —— 快照形式。
    ///
    /// 规范序 = `(at_ps, seq)` 升序，与实际出队顺序逐条一致；用它做快照
    /// 可以保证「恢复后继续推进」与「未恢复时继续推进」产生完全相同的
    /// 事件序列（D-05 / FR-12）。
    pub fn to_ordered_vec(&self) -> Vec<QueuedEvent> {
        let mut events: Vec<QueuedEvent> = self.heap.iter().cloned().collect();
        events.sort_by_key(|e| e.sort_key());
        events
    }

    /// 由规范出队序重建队列。
    ///
    /// 重建后的出队序列与传入顺序逐条一致：`Ord` 只看 `(at_ps, seq)`，
    /// 而 `seq` 在队列内唯一，所以堆的内部布局不影响出队结果。
    pub fn from_ordered_vec(events: Vec<QueuedEvent>) -> Self {
        let mut queue = Self::new();
        for event in events {
            queue.push(event);
        }
        queue
    }

    /// 遍历队列中的事件（**堆序**，不是规范出队序）。
    ///
    /// 只用于「队列里有没有某种事件」这类查询——那种查询与顺序无关，
    /// 不值得为此排一次序。需要规范出队序时用 [`EventQueue::to_ordered_vec`]。
    pub fn iter(&self) -> impl Iterator<Item = &QueuedEvent> {
        self.heap.iter()
    }

    /// 队列中已占用的最大插入序（None = 空队列）。
    ///
    /// 恢复时用它把插入序计数器顶到已占用值之上，避免撞号（D-17）。
    pub fn max_seq(&self) -> Option<u64> {
        self.heap.iter().map(|e| e.seq).max()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(at_ps: u64, seq: u64) -> QueuedEvent {
        QueuedEvent {
            at_ps,
            seq,
            event: SchedEvent::TimerExpiry { irq: 0, at_ps },
        }
    }

    #[test]
    fn pops_earliest_first() {
        let mut q = EventQueue::new();
        q.push(ev(300, 0));
        q.push(ev(100, 1));
        q.push(ev(200, 2));
        assert_eq!(q.pop().map(|e| e.at_ps), Some(100));
        assert_eq!(q.pop().map(|e| e.at_ps), Some(200));
        assert_eq!(q.pop().map(|e| e.at_ps), Some(300));
        assert!(q.pop().is_none());
    }

    #[test]
    fn same_time_breaks_tie_by_insertion_seq() {
        let mut q = EventQueue::new();
        q.push(ev(100, 7));
        q.push(ev(100, 3));
        q.push(ev(100, 5));
        let order: Vec<u64> = std::iter::from_fn(|| q.pop()).map(|e| e.seq).collect();
        assert_eq!(order, vec![3, 5, 7]);
    }

    #[test]
    fn ordered_vec_is_canonical_pop_order() {
        let mut q = EventQueue::new();
        q.push(ev(300, 0));
        q.push(ev(100, 1));
        q.push(ev(100, 2));
        let ordered = q.to_ordered_vec();
        assert_eq!(
            ordered.iter().map(|e| e.sort_key()).collect::<Vec<_>>(),
            vec![(100, 1), (100, 2), (300, 0)]
        );
        // 重建后出队序列逐条一致
        let mut rebuilt = EventQueue::from_ordered_vec(ordered);
        let popped: Vec<(u64, u64)> = std::iter::from_fn(|| rebuilt.pop())
            .map(|e| e.sort_key())
            .collect();
        assert_eq!(popped, vec![(100, 1), (100, 2), (300, 0)]);
    }

    #[test]
    fn max_seq_reports_highest_insertion_seq() {
        let mut q = EventQueue::new();
        assert_eq!(q.max_seq(), None);
        q.push(ev(1, 4));
        q.push(ev(2, 9));
        assert_eq!(q.max_seq(), Some(9));
    }
}
