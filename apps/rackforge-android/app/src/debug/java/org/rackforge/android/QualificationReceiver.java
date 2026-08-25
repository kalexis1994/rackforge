package org.rackforge.android;

import android.content.BroadcastReceiver;
import android.content.Context;
import android.content.Intent;
import android.util.Log;
import java.io.FileOutputStream;
import java.nio.charset.StandardCharsets;

/** Debug-only ADB bridge for deterministic hardware qualification snapshots. */
public final class QualificationReceiver extends BroadcastReceiver {
    static final String ACTION = "org.rackforge.android.action.QUALIFICATION_SNAPSHOT";
    static final String MIDI_PULSE_ACTION =
            "org.rackforge.android.action.QUALIFICATION_MIDI_PULSE";
    static final String SNAPSHOT_FILE = "rackforge-qualification.json";

    @Override
    public void onReceive(Context context, Intent intent) {
        if (intent == null) {
            setResultCode(2);
            return;
        }
        if (MIDI_PULSE_ACTION.equals(intent.getAction())) {
            boolean accepted = MainActivity.qualificationMidiPulse(
                    intent.getIntExtra("note", 60),
                    intent.getIntExtra("velocity", 96),
                    intent.getLongExtra("duration_ms", 250L));
            setResultCode(accepted ? 0 : 1);
            setResultData(accepted ? "midi-pulse-accepted" : "audio-not-ready");
            return;
        }
        if (!ACTION.equals(intent.getAction())) {
            setResultCode(2);
            return;
        }
        String snapshot = MainActivity.qualificationSnapshot();
        try (FileOutputStream output = context.openFileOutput(
                SNAPSHOT_FILE, Context.MODE_PRIVATE)) {
            output.write(snapshot.getBytes(StandardCharsets.UTF_8));
            output.flush();
            setResultCode(0);
            setResultData("snapshot-ready");
        } catch (Exception error) {
            Log.e("RackForgeQualification", "Could not persist snapshot", error);
            setResultCode(1);
            setResultData(error.toString());
        }
    }
}
