// Compile SynthDefs for tui-mixer
// Run: sclang mixerChannel.scd
(
SynthDef("mixerChannel", { |in=2, out=0, vol=0.8, lpf=20000, hpf=20, eqLow=0, eqMid=0, eqHigh=0, pan=0|
    var sig = In.ar(in, 2);
    sig = BPeakEQ.ar(sig, 100, 0.7, eqLow);
    sig = BPeakEQ.ar(sig, 1000, 0.7, eqMid);
    sig = BPeakEQ.ar(sig, 8000, 0.7, eqHigh);
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
