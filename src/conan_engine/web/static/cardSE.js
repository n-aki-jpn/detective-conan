class CardGameSE {
  constructor() {
    this.ctx = null;
    this.noiseBuffer = null;
    this.masterGain = null;
    this.volume = 0.7;
    this.muted = false;
  }

  _syncMasterGain() {
    if (this.masterGain && this.ctx) {
      this.masterGain.gain.setValueAtTime(
        this.muted ? 0 : this.volume,
        this.ctx.currentTime
      );
    }
  }

  setVolume(vol) {
    const parsed = Number(vol);
    if (!Number.isFinite(parsed)) return;
    this.volume = Math.max(0, Math.min(1, parsed));
    this._syncMasterGain();
  }

  setMuted(muted) {
    this.muted = Boolean(muted);
    this._syncMasterGain();
  }

  // 初回のユーザー操作時にAudioContextを初期化・再開
  _init() {
    if (typeof window === 'undefined') return false;
    const AudioCtx = window.AudioContext || window.webkitAudioContext;
    if (!AudioCtx) return false;
    try {
      if (!this.ctx) {
        this.ctx = new AudioCtx();
        this.masterGain = this.ctx.createGain();
        this.masterGain.connect(this.ctx.destination);
        this._createNoiseBuffer();
      }
      this._syncMasterGain();
      if (this.ctx.state === 'suspended' && typeof this.ctx.resume === 'function') {
        this.ctx.resume().catch(() => {});
      }
      return Boolean(this.ctx && this.masterGain && this.noiseBuffer);
    } catch (_e) {
      return false;
    }
  }

  // 紙の擦れ音・シャッフル音用のホワイトノイズを事前生成
  _createNoiseBuffer() {
    const bufferSize = this.ctx.sampleRate * 1.0;
    const buffer = this.ctx.createBuffer(1, bufferSize, this.ctx.sampleRate);
    const data = buffer.getChannelData(0);
    for (let i = 0; i < bufferSize; i++) {
      data[i] = Math.random() * 2 - 1;
    }
    this.noiseBuffer = buffer;
  }

  // 毎回わずかにピッチを揺らすヘルパー（単調さ防止）
  _vary(base, range = 0.06) {
    return base * (1 + (Math.random() * 2 - 1) * range);
  }

  // ① ドロー・手札配布（「シュッ」という紙が滑る音）
  draw() {
    if (this.muted || this.volume <= 0) return;
    if (!this._init()) return;
    const t = this.ctx.currentTime;

    const src = this.ctx.createBufferSource();
    src.buffer = this.noiseBuffer;

    const filter = this.ctx.createBiquadFilter();
    filter.type = 'bandpass';
    filter.frequency.setValueAtTime(this._vary(1400), t);
    filter.frequency.exponentialRampToValueAtTime(this._vary(3200), t + 0.08);
    filter.Q.value = 1.8;

    const gain = this.ctx.createGain();
    gain.gain.setValueAtTime(0.01, t);
    gain.gain.linearRampToValueAtTime(0.22, t + 0.02);
    gain.gain.exponentialRampToValueAtTime(0.001, t + 0.09);

    src.connect(filter).connect(gain).connect(this.masterGain);
    src.start(t);
    src.stop(t + 0.1);
  }

  // ② 場・FILE・証拠エリアへの配置（「パシッ／トンッ」という着地音）
  place() {
    if (this.muted || this.volume <= 0) return;
    if (!this._init()) return;
    const t = this.ctx.currentTime;

    // 紙のインパクト音（ノイズ）
    const src = this.ctx.createBufferSource();
    src.buffer = this.noiseBuffer;

    const filter = this.ctx.createBiquadFilter();
    filter.type = 'lowpass';
    filter.frequency.setValueAtTime(this._vary(2600), t);
    filter.frequency.exponentialRampToValueAtTime(600, t + 0.06);

    const nGain = this.ctx.createGain();
    nGain.gain.setValueAtTime(0.3, t);
    nGain.gain.exponentialRampToValueAtTime(0.001, t + 0.065);

    src.connect(filter).connect(nGain).connect(this.masterGain);
    src.start(t);
    src.stop(t + 0.07);

    // 卓面の低い響き（サイン波）
    const osc = this.ctx.createOscillator();
    osc.type = 'triangle';
    osc.frequency.setValueAtTime(this._vary(150), t);
    osc.frequency.exponentialRampToValueAtTime(55, t + 0.06);

    const oGain = this.ctx.createGain();
    oGain.gain.setValueAtTime(0.28, t);
    oGain.gain.exponentialRampToValueAtTime(0.001, t + 0.065);

    osc.connect(oGain).connect(this.masterGain);
    osc.start(t);
    osc.stop(t + 0.07);
  }

  // ③ マリガン確定・シャッフル（「パラパラッ」という連続音）
  shuffle() {
    if (this.muted || this.volume <= 0) return;
    if (!this._init()) return;
    const count = 6;
    for (let i = 0; i < count; i++) {
      const delay = i * 0.032 + Math.random() * 0.01;
      const t = this.ctx.currentTime + delay;

      const src = this.ctx.createBufferSource();
      src.buffer = this.noiseBuffer;

      const filter = this.ctx.createBiquadFilter();
      filter.type = 'bandpass';
      filter.frequency.setValueAtTime(this._vary(2200, 0.12), t);
      filter.Q.value = 2.2;

      const gain = this.ctx.createGain();
      gain.gain.setValueAtTime(0.18, t);
      gain.gain.exponentialRampToValueAtTime(0.001, t + 0.035);

      src.connect(filter).connect(gain).connect(this.masterGain);
      src.start(t);
      src.stop(t + 0.04);
    }
  }

  // ④ 手札タップ・ボタン選択（軽いUIクリック音）
  tap() {
    if (this.muted || this.volume <= 0) return;
    if (!this._init()) return;
    const t = this.ctx.currentTime;

    const osc = this.ctx.createOscillator();
    osc.type = 'sine';
    osc.frequency.setValueAtTime(this._vary(780, 0.03), t);
    osc.frequency.exponentialRampToValueAtTime(1150, t + 0.035);

    const gain = this.ctx.createGain();
    gain.gain.setValueAtTime(0.14, t);
    gain.gain.exponentialRampToValueAtTime(0.001, t + 0.04);

    osc.connect(gain).connect(this.masterGain);
    osc.start(t);
    osc.stop(t + 0.045);
  }

  // ⑤ 詳細パネル・選択パネル開閉（「シュッ」というウィンドウ展開音）
  panel() {
    if (this.muted || this.volume <= 0) return;
    if (!this._init()) return;
    const t = this.ctx.currentTime;

    const osc = this.ctx.createOscillator();
    osc.type = 'sine';
    osc.frequency.setValueAtTime(320, t);
    osc.frequency.exponentialRampToValueAtTime(640, t + 0.07);

    const gain = this.ctx.createGain();
    gain.gain.setValueAtTime(0.01, t);
    gain.gain.linearRampToValueAtTime(0.12, t + 0.015);
    gain.gain.exponentialRampToValueAtTime(0.001, t + 0.08);

    osc.connect(gain).connect(this.masterGain);
    osc.start(t);
    osc.stop(t + 0.085);
  }

  // ⑥ 自分のターン開始（「⚡操作中」切り替わり時のクールな通知音）
  turnStart() {
    if (this.muted || this.volume <= 0) return;
    if (!this._init()) return;
    const notes = [587.33, 880]; // D5 -> A5
    notes.forEach((freq, i) => {
      const t = this.ctx.currentTime + i * 0.075;
      const osc = this.ctx.createOscillator();
      osc.type = 'triangle';
      osc.frequency.setValueAtTime(freq, t);

      const gain = this.ctx.createGain();
      gain.gain.setValueAtTime(0.01, t);
      gain.gain.linearRampToValueAtTime(0.18, t + 0.01);
      gain.gain.exponentialRampToValueAtTime(0.001, t + 0.22);

      osc.connect(gain).connect(this.masterGain);
      osc.start(t);
      osc.stop(t + 0.24);
    });
  }

  // ⑦ 推理・証拠獲得・効果発動（閃きチャイム）
  resolve() {
    if (this.muted || this.volume <= 0) return;
    if (!this._init()) return;
    const notes = [659.25, 830.61, 987.77, 1318.51]; // E5, G#5, B5, E6
    notes.forEach((freq, i) => {
      const t = this.ctx.currentTime + i * 0.045;
      const osc = this.ctx.createOscillator();
      osc.type = 'sine';
      osc.frequency.setValueAtTime(freq, t);

      const gain = this.ctx.createGain();
      gain.gain.setValueAtTime(0.14, t);
      gain.gain.exponentialRampToValueAtTime(0.001, t + 0.25);

      osc.connect(gain).connect(this.masterGain);
      osc.start(t);
      osc.stop(t + 0.26);
    });
  }
}

const se = new CardGameSE();
if (typeof window !== 'undefined') {
  window.CardGameSE = CardGameSE;
  window.se = se;
}
if (typeof module !== 'undefined' && module.exports) {
  module.exports = { CardGameSE, se };
}
