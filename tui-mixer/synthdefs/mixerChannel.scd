// Compile SynthDefs for tui-mixer
// Run: sclang mixerChannel.scd
(
SynthDef("mixerChannel", { |in=2, out=0, vol=0.8, lpf=20000, hpf=20, eqLowFreq=20000, eqMidFreq=1000, eqMidGain=0, eqHighFreq=20,
    mEq32=0, mEq64=0, mEq125=0, mEq250=0, mEq500=0,
    mEq1k=0, mEq2k=0, mEq4k=0, mEq8k=0, mEq16k=0,
    pan=0|
    var sig = In.ar(in, 2);
    // Per-channel EQ: lowpass for L, sweepable peak for M, highpass for H
    sig = LPF.ar(sig, eqLowFreq);
    sig = BPeakEQ.ar(sig, eqMidFreq, 0.7, eqMidGain);
    sig = HPF.ar(sig, eqHighFreq);
    // 10-band master EQ
    sig = BPeakEQ.ar(sig, 32, 1.0, mEq32);
    sig = BPeakEQ.ar(sig, 64, 1.0, mEq64);
    sig = BPeakEQ.ar(sig, 125, 1.0, mEq125);
    sig = BPeakEQ.ar(sig, 250, 1.0, mEq250);
    sig = BPeakEQ.ar(sig, 500, 1.0, mEq500);
    sig = BPeakEQ.ar(sig, 1000, 1.0, mEq1k);
    sig = BPeakEQ.ar(sig, 2000, 1.0, mEq2k);
    sig = BPeakEQ.ar(sig, 4000, 1.0, mEq4k);
    sig = BPeakEQ.ar(sig, 8000, 1.0, mEq8k);
    sig = BPeakEQ.ar(sig, 16000, 1.0, mEq16k);
    sig = LPF.ar(sig, lpf);
    sig = HPF.ar(sig, hpf);
    sig = sig * vol;
    sig = Balance2.ar(sig[0], sig[1], pan);
    Out.ar(out, sig);
}).writeDefFile("/Users/dwtompkins/Downloads/Tidal/tui-mixer/synthdefs");

SynthDef("monitorChannel", { |in=0, out=2|
    var sig = In.ar(in, 2);
    Out.ar(out, sig);
}).writeDefFile("/Users/dwtompkins/Downloads/Tidal/tui-mixer/synthdefs");

"SynthDefs written!".postln;
0.exit;
)
