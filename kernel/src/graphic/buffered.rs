use core::sync::atomic::{AtomicBool, Ordering};

use crate::memory_manager::Mutex;

use super::frame_buffer::FrameBuffer;

/// 書き込み用のFrameBufferと読み出し用のFrameBufferを合わせたキャンバス
/// 書き込みスレッドと読み出しスレッドの間でロックの取り合いが起こるのを防ぐ
/// foreとback両方をロックするのはflushのみであり、flushはmemcpyでforeからbackへのコピーを行う
pub struct BufferedCanvas {
    /// 読み出し用のFrameBuffer
    fore: Mutex<FrameBuffer>,
    /// まず最初に書き込みを受けるFrameBuffer
    back: Mutex<FrameBuffer>,
    /// 部分描画用のフラグ
    is_updated: AtomicBool
}

impl BufferedCanvas {
    pub fn new(width: usize, height: usize) -> Self {
        Self { fore: Mutex::new(FrameBuffer::new(width, height)), back: Mutex::new(FrameBuffer::new(width, height)), is_updated: AtomicBool::new(false)}
    }
    /// backからforeへのコピー
    /// foreとback両方のlockを取る
    pub fn flush(&self) {
        self.fore.lock().copy((0,0).into(), &self.back.lock());
    }

    /// foreのlockを取り、fを実行
    pub fn with_fore(&self, f: impl FnOnce(&FrameBuffer)) {
        f(&self.fore.lock());
    }

    /// backのlockを取り、draw_funcを実行
    pub fn write_with(&self, draw_func: impl FnOnce(&mut FrameBuffer)) {
        draw_func(&mut self.back.lock());
        self.is_updated.store(true, Ordering::Relaxed);
    }

    pub fn is_updated(&self) -> bool {
        self.is_updated.load(Ordering::Relaxed)
    }

    pub fn clear_update_flag(&self) {
        self.is_updated.store(false, Ordering::Relaxed);
    }
}
