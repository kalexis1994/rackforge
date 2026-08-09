package org.rackforge.android;

import android.app.Activity;
import android.app.AlertDialog;
import android.app.Dialog;
import android.content.Context;
import android.content.Intent;
import android.content.SharedPreferences;
import android.database.Cursor;
import android.net.Uri;
import android.content.pm.PackageManager;
import android.graphics.Color;
import android.graphics.drawable.ColorDrawable;
import android.graphics.drawable.GradientDrawable;
import android.hardware.usb.UsbDevice;
import android.hardware.usb.UsbManager;
import android.media.AudioDeviceCallback;
import android.media.AudioDeviceInfo;
import android.media.AudioManager;
import android.media.midi.MidiDevice;
import android.media.midi.MidiDeviceInfo;
import android.media.midi.MidiInputPort;
import android.media.midi.MidiManager;
import android.media.midi.MidiOutputPort;
import android.media.midi.MidiReceiver;
import android.os.Build;
import android.os.Bundle;
import android.os.Handler;
import android.os.Looper;
import android.os.PowerManager;
import android.provider.OpenableColumns;
import android.util.Log;
import android.webkit.JavascriptInterface;
import android.webkit.MimeTypeMap;
import android.webkit.WebResourceRequest;
import android.webkit.WebResourceResponse;
import android.view.View;
import android.view.ViewGroup;
import android.view.Window;
import android.view.WindowInsets;
import android.webkit.WebSettings;
import android.webkit.WebView;
import android.webkit.WebViewClient;
import android.widget.ArrayAdapter;
import android.widget.Button;
import android.widget.CheckBox;
import android.widget.LinearLayout;
import android.widget.PopupMenu;
import android.widget.ScrollView;
import android.widget.Spinner;
import android.widget.Switch;
import android.widget.TextView;
import android.widget.Toast;

import java.io.ByteArrayOutputStream;
import java.io.File;
import java.io.FileInputStream;
import java.io.FileOutputStream;
import java.io.InputStream;
import java.security.MessageDigest;
import java.util.ArrayList;
import java.util.Comparator;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.Set;
import java.util.concurrent.atomic.AtomicInteger;

import org.json.JSONObject;
import org.json.JSONArray;

public final class MainActivity extends Activity {
    static {
        System.loadLibrary("rackforge_android");
    }

    private static final int SAMPLE_RATE = 48_000;
    private static final int REQUEST_INSTALL_PLUGIN = 4101;
    private static final long MAX_PLUGIN_BYTES = 512L * 1024L * 1024L;
    private WebView webView;
    private Spinner audioOutputSpinner;
    private volatile boolean audioRunning;
    private volatile int selectedAudioDeviceId;
    private String selectedAudioDeviceKey = "default";
    private int latencyMode;
    private int outputGainDb;
    private boolean refreshingAudioOutputs;
    private final Handler mainHandler = new Handler(Looper.getMainLooper());
    private final List<AudioOutputChoice> audioOutputChoices = new ArrayList<>();
    private final List<MidiDevice> openMidiDevices = new ArrayList<>();
    private final List<MidiOutputPort> openMidiPorts = new ArrayList<>();
    private final List<MidiInputPort> openMidiDestinations = new ArrayList<>();
    private final List<MidiInputPort> openKeyLabDestinations = new ArrayList<>();
    private AudioDeviceCallback audioDeviceCallback;
    private MidiManager.DeviceCallback midiDeviceCallback;
    private volatile int midiGeneration;
    private final AtomicInteger keyLabHeaderGeneration = new AtomicInteger();
    private volatile boolean audioRecoveryInProgress;
    private ThermalMonitor thermalMonitor;
    private int thermalStatus = PowerManager.THERMAL_STATUS_NONE;
    private SharedPreferences preferences;
    private String currentPage = "play";
    private volatile boolean engineStarting;
    private File pluginPackageRoot;
    private String pluginWebEntry;
    private String activePluginName = "Plugin";
    private String activePluginVersion = "";
    private TextView activePluginLabel;
    private LinearLayout playToolbar;
    private TextView playContextLabel;
    private AlertDialog pluginPickerDialog;
    private AlertDialog installedPluginsDialog;

    private static native boolean initializeEngine(byte[] archive, String storeRoot, String dataRoot);
    private static native String installPluginFile(String archivePath, String storeRoot);
    private static native String installedPlugins(String storeRoot);
    private static native boolean activateInstalledPlugin(String packageRoot, String storeRoot, String dataRoot);
    private static native String pluginPackageRoot();
    private static native String pluginWebEntry();
    private static native String pluginWebContext();
    private static native boolean selectPluginSound(String soundId);
    private static native void sendMidiMessage(int status, int data1, int data2, int length);
    private static native void releaseMidiNotes();
    private static native String keyLabAcquirePlan();
    private static native String keyLabRestorePlan();
    private static native String keyLabHandleMidi(int status, int data1, int data2);
    private static native String keyLabPollLongPress();
    private static native boolean keyLabSyncPlugins(String storeRoot);
    private static native boolean keyLabSyncActivePlugin();
    private static native String keyLabRenderPlan();
    private static native boolean startNativeAudio(int deviceId, int latencyMode);
    private static native void setNativeOutputGain(int gainDb);
    static native void stopNativeAudio();
    private static native String nativeAudioStatus();
    private static native int pollNativeAudioError();

    private final Runnable audioHealthPoll = new Runnable() {
        @Override public void run() {
            if (audioRunning) {
                int error = pollNativeAudioError();
                if (error != 0) recoverAudioStream(error);
            }
            mainHandler.postDelayed(this, 1_000);
        }
    };

    private final Runnable midiReconnect = () -> {
        if (!audioRunning || engineStarting) return;
        closeMidi();
        openMidiInputs();
        if ("live".equals(currentPage)) showLive();
        else if ("diagnostics".equals(currentPage)) renderDiagnostics();
    };

    @Override
    protected void onCreate(Bundle state) {
        super.onCreate(state);
        preferences = getSharedPreferences("rackforge-settings", MODE_PRIVATE);
        selectedAudioDeviceKey = preferences.getString("audio.output", "default");
        latencyMode = preferences.getInt("audio.latency", 0);
        outputGainDb = preferences.getInt("audio.gain_db", 0);
        setNativeOutputGain(outputGainDb);
        webView = new WebView(this);
        WebSettings settings = webView.getSettings();
        settings.setJavaScriptEnabled(true);
        settings.setAllowFileAccess(false);
        settings.setAllowContentAccess(false);
        settings.setDomStorageEnabled(false);
        webView.setBackgroundColor(0xFF050F16);
        webView.addJavascriptInterface(new PluginWebBridge(), "RackForgeAndroid");
        webView.setWebViewClient(pluginWebViewClient());

        audioOutputSpinner = new Spinner(this);

        LinearLayout root = new LinearLayout(this);
        root.setOrientation(LinearLayout.VERTICAL);
        root.setBackgroundColor(0xFF050F16);
        root.addView(buildTopBar(), new LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT));
        root.addView(buildPlayToolbar(), new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, dp(58)));
        root.addView(webView, new LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1));
        setContentView(root);
        refreshAudioOutputs();
        registerAudioDeviceUpdates();
        registerMidiDeviceUpdates();
        registerThermalMonitoring();
        mainHandler.post(audioHealthPoll);
        showPlay();
        startEngine();
    }

    @Override
    protected void onResume() {
        super.onResume();
        refreshAudioOutputs();
        scheduleMidiReconnect();
        if ("diagnostics".equals(currentPage)) renderDiagnostics();
        else if ("live".equals(currentPage)) showLive();
        else if ("idle".equals(currentPage)) showIdle();
        else showPlay();
    }

    @Override
    protected void onDestroy() {
        audioRunning = false;
        stopNativeAudio();
        stopService(new Intent(this, AudioEngineService.class));
        mainHandler.removeCallbacksAndMessages(null);
        AudioManager audioManager = (AudioManager) getSystemService(Context.AUDIO_SERVICE);
        if (audioDeviceCallback != null) audioManager.unregisterAudioDeviceCallback(audioDeviceCallback);
        MidiManager midiManager = (MidiManager) getSystemService(Context.MIDI_SERVICE);
        if (midiManager != null && midiDeviceCallback != null) {
            midiManager.unregisterDeviceCallback(midiDeviceCallback);
        }
        if (thermalMonitor != null) thermalMonitor.stop();
        closeMidi();
        webView.destroy();
        super.onDestroy();
    }

    private WebViewClient pluginWebViewClient() {
        return new WebViewClient() {
            @Override
            public boolean shouldOverrideUrlLoading(WebView view, WebResourceRequest request) {
                return !"rackforge.local".equals(request.getUrl().getHost());
            }

            @Override
            public WebResourceResponse shouldInterceptRequest(WebView view, WebResourceRequest request) {
                String path = request.getUrl().getPath();
                if (pluginPackageRoot == null || path == null || !path.startsWith("/plugin/")) {
                    return null;
                }
                try {
                    String relative = path.substring("/plugin/".length());
                    File root = pluginPackageRoot.getCanonicalFile();
                    File asset = new File(root, relative).getCanonicalFile();
                    if (!asset.getPath().startsWith(root.getPath() + File.separator) || !asset.isFile()) {
                        return new WebResourceResponse("text/plain", "UTF-8", 404, "Not Found",
                                java.util.Collections.emptyMap(),
                                new java.io.ByteArrayInputStream(new byte[0]));
                    }
                    String extension = MimeTypeMap.getFileExtensionFromUrl(asset.getName());
                    String mime = MimeTypeMap.getSingleton().getMimeTypeFromExtension(extension);
                    if (mime == null) mime = "application/octet-stream";
                    return new WebResourceResponse(mime, "UTF-8", new FileInputStream(asset));
                } catch (Exception error) {
                    Log.e("RackForge", "Could not serve plugin Web asset", error);
                    return null;
                }
            }

            @Override
            public void onPageFinished(WebView view, String url) {
                if (url.startsWith("https://rackforge.local/plugin/")) {
                    view.scrollTo(0, 0);
                    injectPluginBridge();
                }
            }
        };
    }

    private void injectPluginBridge() {
        String script = "(function(){"
                + "if(!window.__rackforgeAndroidBridge){"
                + "window.__rackforgeAndroidBridge=true;"
                + "window.addEventListener('message',function(event){"
                + "var data=event.data;"
                + "if(data&&data.protocol==='rackforge.plugin.web@1'&&data.kind==='request'){"
                + "RackForgeAndroid.postMessage(JSON.stringify(data));"
                + "}});"
                + "}"
                + "window.postMessage(" + pluginWebContext() + ",window.location.origin);"
                + "})();";
        webView.evaluateJavascript(script, null);
    }

    private void sendPluginMessage(String json) {
        webView.evaluateJavascript(
                "window.postMessage(" + json + ",window.location.origin);", null);
    }

    private final class PluginWebBridge {
        @JavascriptInterface
        public void postMessage(String payload) {
            try {
                JSONObject request = new JSONObject(payload);
                if (!"request".equals(request.optString("kind"))) return;
                String requestId = request.getString("request_id");
                String method = request.optString("method");
                if (!"plugin.select_sound".equals(method)) {
                    respondToPlugin(requestId, false, "Method is not available on Android.");
                    return;
                }
                String soundId = request.getJSONObject("params").getString("sound_id");
                new Thread(() -> {
                    try {
                        if (!selectPluginSound(soundId)) {
                            throw new IllegalStateException("The plugin rejected the selected sound");
                        }
                        if (!keyLabSyncActivePlugin()) {
                            throw new IllegalStateException("The controller did not accept the selected sound");
                        }
                        refreshKeyLabDisplay();
                        Log.i("RackForge", "Selected plugin sound " + soundId);
                        runOnUiThread(() -> {
                            respondToPlugin(requestId, true, null);
                            sendPluginMessage(pluginWebContext());
                        });
                    } catch (Throwable error) {
                        Log.e("RackForge", "Plugin Web command failed", error);
                        runOnUiThread(() -> respondToPlugin(requestId, false, error.getMessage()));
                    }
                }, "rackforge-plugin-web-command").start();
            } catch (Throwable error) {
                Log.e("RackForge", "Invalid plugin Web message", error);
            }
        }
    }

    private void respondToPlugin(String requestId, boolean ok, String error) {
        try {
            JSONObject response = new JSONObject();
            response.put("protocol", "rackforge.plugin.web@1");
            response.put("kind", "response");
            response.put("request_id", requestId);
            response.put("ok", ok);
            if (error != null) response.put("error", error);
            sendPluginMessage(response.toString());
        } catch (Exception exception) {
            Log.e("RackForge", "Could not answer plugin Web message", exception);
        }
    }

    private LinearLayout buildTopBar() {
        LinearLayout bar = new LinearLayout(this);
        bar.setGravity(android.view.Gravity.CENTER_VERTICAL);
        bar.setPadding(dp(12), dp(8), dp(12), dp(8));
        bar.setBackgroundColor(0xFF0D202A);
        bar.setOnApplyWindowInsetsListener((view, insets) -> {
            int statusTop = Build.VERSION.SDK_INT >= Build.VERSION_CODES.R
                    ? insets.getInsets(WindowInsets.Type.statusBars()).top
                    : insets.getSystemWindowInsetTop();
            view.setPadding(dp(12), dp(8) + statusTop, dp(12), dp(8));
            return insets;
        });
        bar.addView(toolbarButton("☰", this::showMainMenu));
        TextView title = new TextView(this);
        title.setText("RACKFORGE");
        title.setTextColor(0xFF5CE2F5);
        title.setTextSize(18);
        title.setTypeface(null, android.graphics.Typeface.BOLD);
        title.setPadding(dp(8), 0, dp(12), 0);
        bar.addView(title, new LinearLayout.LayoutParams(
                0, ViewGroup.LayoutParams.WRAP_CONTENT, 1));
        activePluginLabel = new TextView(this);
        activePluginLabel.setText(activePluginDisplayName());
        activePluginLabel.setTextColor(0xFFB8CDD3);
        activePluginLabel.setPadding(dp(8), 0, dp(8), 0);
        bar.addView(activePluginLabel);
        bar.addView(toolbarButton("⚙", view -> showSettingsDialog()));
        return bar;
    }

    private LinearLayout buildPlayToolbar() {
        playToolbar = new LinearLayout(this);
        playToolbar.setGravity(android.view.Gravity.CENTER_VERTICAL);
        playToolbar.setPadding(dp(16), dp(7), dp(12), dp(7));
        playToolbar.setBackgroundColor(0xFF081820);

        playContextLabel = new TextView(this);
        playContextLabel.setText("PLAY · " + activePluginDisplayName());
        playContextLabel.setTextColor(0xFFF2FAFC);
        playContextLabel.setTextSize(13);
        playContextLabel.setTypeface(null, android.graphics.Typeface.BOLD);
        playContextLabel.setSingleLine(true);
        playContextLabel.setEllipsize(android.text.TextUtils.TruncateAt.END);
        playToolbar.addView(playContextLabel, new LinearLayout.LayoutParams(
                0, ViewGroup.LayoutParams.WRAP_CONTENT, 1));

        Button select = toolbarButton("▦  Select plugin", view -> showPluginPickerDialog());
        select.setTextColor(0xFF5CE2F5);
        playToolbar.addView(select, new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.WRAP_CONTENT, ViewGroup.LayoutParams.MATCH_PARENT));
        updateModeButtons();
        return playToolbar;
    }

    private void showPluginPickerDialog() {
        if (pluginPickerDialog != null && pluginPickerDialog.isShowing()) {
            pluginPickerDialog.dismiss();
        }
        ScrollView scroll = new ScrollView(this);
        scroll.setFillViewport(true);
        LinearLayout content = new LinearLayout(this);
        content.setOrientation(LinearLayout.VERTICAL);
        content.setPadding(dp(20), dp(22), dp(20), dp(28));
        scroll.addView(content);

        TextView title = new TextView(this);
        title.setText("Select plugin");
        title.setTextColor(0xFFF2FAFC);
        title.setTextSize(27);
        title.setTypeface(null, android.graphics.Typeface.BOLD);
        content.addView(title);
        TextView subtitle = new TextView(this);
        subtitle.setText("Choose the instrument you want to play");
        subtitle.setTextColor(0xFF91A9B1);
        subtitle.setTextSize(13);
        subtitle.setPadding(0, dp(2), 0, dp(16));
        content.addView(subtitle);

        LinearLayout cards = new LinearLayout(this);
        cards.setOrientation(LinearLayout.VERTICAL);
        cards.addView(settingsValue("Status", "Loading installed plugins…"));
        content.addView(cards);

        AlertDialog dialog = new AlertDialog.Builder(this)
                .setView(scroll)
                .setPositiveButton("Close", null)
                .create();
        pluginPickerDialog = dialog;
        dialog.setOnDismissListener(unused -> {
            if (pluginPickerDialog == dialog) pluginPickerDialog = null;
        });
        dialog.setOnShowListener(unused -> styleFullHeightDialog(dialog));
        dialog.show();

        new Thread(() -> {
            try {
                JSONObject catalog = new JSONObject(installedPlugins(pluginStoreRoot().getAbsolutePath()));
                JSONArray plugins = catalog.getJSONArray("plugins");
                runOnUiThread(() -> renderPluginPickerCards(dialog, cards, plugins));
            } catch (Throwable error) {
                Log.e("RackForge", "Could not list Play instruments", error);
                runOnUiThread(() -> {
                    if (!dialog.isShowing()) return;
                    cards.removeAllViews();
                    cards.addView(settingsValue("Status", "Installed plugins are unavailable"));
                });
            }
        }, "rackforge-plugin-picker").start();
    }

    private void renderPluginPickerCards(AlertDialog dialog, LinearLayout cards, JSONArray plugins) {
        if (!dialog.isShowing()) return;
        try {
            cards.removeAllViews();
            for (int index = 0; index < plugins.length(); index++) {
                View card = pluginPickerCard(dialog, plugins.getJSONObject(index));
                LinearLayout.LayoutParams params = new LinearLayout.LayoutParams(
                        ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT);
                params.bottomMargin = dp(10);
                cards.addView(card, params);
            }
            if (plugins.length() == 0) {
                cards.addView(settingsValue("Status", "No plugins installed"));
            }
        } catch (Throwable error) {
            Log.e("RackForge", "Could not render plugin picker", error);
            cards.removeAllViews();
            cards.addView(settingsValue("Status", "Plugin list could not be rendered"));
        }
    }

    private View pluginPickerCard(AlertDialog dialog, JSONObject plugin) throws Exception {
        boolean active = plugin.optBoolean("active");
        boolean compatible = plugin.optBoolean("compatible");
        String name = plugin.getString("plugin_name");
        String version = plugin.getString("version");

        LinearLayout card = new LinearLayout(this);
        card.setOrientation(LinearLayout.VERTICAL);
        card.setGravity(android.view.Gravity.CENTER_VERTICAL);
        card.setPadding(dp(18), dp(16), dp(18), dp(16));
        card.setBackground(surface(active ? 0xFF153B46 : 0xFF10252E, 16,
                active ? 0xFF5CE2F5 : 0xFF244650, active ? 2 : 1));

        TextView title = new TextView(this);
        title.setText(name);
        title.setTextColor(compatible ? 0xFFF2FAFC : 0xFF718991);
        title.setTextSize(18);
        title.setTypeface(null, android.graphics.Typeface.BOLD);
        title.setSingleLine(true);
        title.setEllipsize(android.text.TextUtils.TruncateAt.END);
        card.addView(title, new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT));

        TextView detail = new TextView(this);
        detail.setText(active ? "●  ACTIVE · " + versionLabel(version)
                : compatible ? "TAP TO SELECT · " + versionLabel(version)
                : "UNAVAILABLE · " + versionLabel(version));
        detail.setTextColor(active ? 0xFF64DCB5 : compatible ? 0xFF91A9B1 : 0xFFF27777);
        detail.setTextSize(11);
        detail.setPadding(0, dp(5), 0, 0);
        detail.setSingleLine(true);
        card.addView(detail, new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT));

        card.setAlpha(compatible ? 1f : 0.62f);
        card.setEnabled(compatible && !engineStarting);
        card.setContentDescription(name + " " + versionLabel(version) + (active ? ", active" : ""));
        if (compatible) {
            String root = plugin.getString("package_root");
            card.setOnClickListener(view -> {
                dialog.dismiss();
                if (active) {
                    showPlay();
                    webView.scrollTo(0, 0);
                } else {
                    activatePlugin(root, name, version);
                }
            });
        }
        return card;
    }

    private void updateModeButtons() {
        if (playToolbar != null) playToolbar.setVisibility(
                "play".equals(currentPage) ? View.VISIBLE : View.GONE);
        if (playContextLabel != null) playContextLabel.setText(
                "PLAY · " + activePluginDisplayName());
    }

    private Button toolbarButton(String text, android.view.View.OnClickListener listener) {
        Button button = new Button(this);
        button.setText(text);
        button.setTextColor(0xFFE2F2F5);
        button.setTextSize(14);
        button.setAllCaps(false);
        button.setMinWidth(0);
        button.setMinimumWidth(0);
        button.setPadding(dp(12), 0, dp(12), 0);
        button.setBackground(surface(0xFF17313D, 10, 0, 0));
        button.setOnClickListener(listener);
        return button;
    }

    private void showMainMenu(View anchor) {
        Dialog menu = new Dialog(this);
        LinearLayout panel = new LinearLayout(this);
        panel.setOrientation(LinearLayout.VERTICAL);
        panel.setPadding(dp(20), dp(14), dp(20), dp(24));
        panel.setBackground(surface(0xFF0D202A, 24, 0xFF2A4B57, 1));

        View handle = new View(this);
        handle.setBackground(surface(0xFF55727C, 3, 0, 0));
        LinearLayout.LayoutParams handleParams = new LinearLayout.LayoutParams(dp(42), dp(4));
        handleParams.gravity = android.view.Gravity.CENTER_HORIZONTAL;
        handleParams.bottomMargin = dp(18);
        panel.addView(handle, handleParams);

        TextView title = new TextView(this);
        title.setText("RackForge");
        title.setTextColor(0xFFF2FAFC);
        title.setTextSize(25);
        title.setTypeface(null, android.graphics.Typeface.BOLD);
        panel.addView(title);
        TextView subtitle = new TextView(this);
        subtitle.setText("Instrument workspace");
        subtitle.setTextColor(0xFF91A9B1);
        subtitle.setTextSize(13);
        subtitle.setPadding(0, 0, 0, dp(18));
        panel.addView(subtitle);

        panel.addView(menuLabel("WORKSPACE"));
        panel.addView(menuAction("▶", "Play", "Play and edit the active instrument", () -> {
            menu.dismiss(); showPlay();
        }));
        panel.addView(menuAction("▦", "Live", "Performance rack, routes and hardware status", () -> {
            menu.dismiss(); showLive();
        }));
        panel.addView(menuAction("⌁", "Diagnostics", "Connected audio, MIDI and USB devices", () -> {
            menu.dismiss(); showDiagnostics();
        }));
        panel.addView(menuLabel("SYSTEM"));
        panel.addView(menuAction("⚙", "Audio & MIDI", "Output, latency, gain and controllers", () -> {
            menu.dismiss(); showSettingsDialog();
        }));
        panel.addView(menuAction("＋", "Install plugin", "Choose a portable .rfplugin package", () -> {
            menu.dismiss(); choosePluginFile();
        }));
        panel.addView(menuAction("▤", "Installed plugins", "Manage versions and active instrument", () -> {
            menu.dismiss(); showInstalledPluginsDialog();
        }));
        panel.addView(menuAction("i", "About RackForge", "Version and runtime information", () -> {
            menu.dismiss(); showAbout();
        }));

        menu.setContentView(panel);
        Window window = menu.getWindow();
        if (window != null) {
            window.setBackgroundDrawable(new ColorDrawable(Color.TRANSPARENT));
            window.setDimAmount(0.72f);
            window.addFlags(android.view.WindowManager.LayoutParams.FLAG_DIM_BEHIND);
            window.setLayout(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT);
            window.setGravity(android.view.Gravity.BOTTOM);
        }
        menu.setOnShowListener(unused -> {
            Window shown = menu.getWindow();
            if (shown != null) shown.setLayout(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT);
        });
        menu.show();
    }

    private TextView menuLabel(String text) {
        TextView label = new TextView(this);
        label.setText(text);
        label.setTextColor(0xFF5CE2F5);
        label.setTextSize(11);
        label.setTypeface(null, android.graphics.Typeface.BOLD);
        label.setPadding(dp(4), dp(14), 0, dp(6));
        return label;
    }

    private View menuAction(String icon, String title, String detail, Runnable action) {
        LinearLayout row = new LinearLayout(this);
        row.setGravity(android.view.Gravity.CENTER_VERTICAL);
        row.setPadding(dp(14), dp(10), dp(14), dp(10));
        row.setBackground(surface(0xFF112B36, 14, 0xFF234752, 1));
        LinearLayout.LayoutParams rowParams = new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT);
        rowParams.bottomMargin = dp(8);
        row.setLayoutParams(rowParams);

        TextView glyph = new TextView(this);
        glyph.setText(icon);
        glyph.setTextColor(action == null ? 0xFF55727C : 0xFF5CE2F5);
        glyph.setTextSize(20);
        glyph.setGravity(android.view.Gravity.CENTER);
        glyph.setBackground(surface(0xFF173946, 12, 0, 0));
        row.addView(glyph, new LinearLayout.LayoutParams(dp(44), dp(44)));

        LinearLayout copy = new LinearLayout(this);
        copy.setOrientation(LinearLayout.VERTICAL);
        copy.setPadding(dp(14), 0, 0, 0);
        TextView heading = new TextView(this);
        heading.setText(title);
        heading.setTextColor(action == null ? 0xFF718991 : 0xFFF2FAFC);
        heading.setTextSize(16);
        heading.setTypeface(null, android.graphics.Typeface.BOLD);
        copy.addView(heading);
        TextView description = new TextView(this);
        description.setText(detail);
        description.setTextColor(action == null ? 0xFF526A72 : 0xFF91A9B1);
        description.setTextSize(12);
        copy.addView(description);
        row.addView(copy, new LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1));
        row.setAlpha(action == null ? 0.6f : 1f);
        row.setEnabled(action != null);
        if (action != null) row.setOnClickListener(view -> action.run());
        return row;
    }

    private void showFileMenu(View anchor) {
        PopupMenu menu = new PopupMenu(this, anchor);
        menu.getMenu().add("Install .rfplugin…").setOnMenuItemClickListener(item -> {
            choosePluginFile();
            return true;
        });
        menu.getMenu().add("Installed plugins").setOnMenuItemClickListener(item -> {
            showInstalledPluginsDialog();
            return true;
        });
        menu.show();
    }

    private void choosePluginFile() {
        Intent intent = new Intent(Intent.ACTION_OPEN_DOCUMENT);
        intent.addCategory(Intent.CATEGORY_OPENABLE);
        intent.setType("application/octet-stream");
        intent.putExtra(Intent.EXTRA_MIME_TYPES, new String[] {
                "application/octet-stream", "application/zip", "application/x-zip-compressed"
        });
        startActivityForResult(intent, REQUEST_INSTALL_PLUGIN);
    }

    @Override
    protected void onActivityResult(int requestCode, int resultCode, Intent data) {
        super.onActivityResult(requestCode, resultCode, data);
        if (requestCode != REQUEST_INSTALL_PLUGIN || resultCode != RESULT_OK || data == null) return;
        Uri uri = data.getData();
        if (uri == null) return;
        try {
            int flags = data.getFlags();
            if ((flags & Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION) != 0
                    && (flags & Intent.FLAG_GRANT_READ_URI_PERMISSION) != 0) {
                getContentResolver().takePersistableUriPermission(
                        uri, Intent.FLAG_GRANT_READ_URI_PERMISSION);
            }
        } catch (SecurityException ignored) {
            // The temporary read grant remains valid for this immediate import.
        }
        importPlugin(uri);
    }

    private void importPlugin(Uri uri) {
        String displayName = selectedFileName(uri);
        if (displayName == null || !displayName.toLowerCase(Locale.ROOT).endsWith(".rfplugin")) {
            Toast.makeText(this, "Choose a file ending in .rfplugin", Toast.LENGTH_LONG).show();
            return;
        }
        Toast.makeText(this, "Validating " + displayName + "…", Toast.LENGTH_SHORT).show();
        new Thread(() -> {
            File temporary = null;
            try {
                temporary = copyPluginToPrivateCache(uri);
                String descriptorText = installPluginFile(
                        temporary.getAbsolutePath(), pluginStoreRoot().getAbsolutePath());
                JSONObject descriptor = new JSONObject(descriptorText);
                String installedName = descriptor.getString("plugin_name");
                String version = descriptor.getString("version");
                boolean alreadyInstalled = descriptor.optBoolean("already_installed");
                keyLabSyncPlugins(pluginStoreRoot().getAbsolutePath());
                refreshKeyLabDisplay();
                runOnUiThread(() -> {
                    Toast.makeText(this,
                            installedName + " " + versionLabel(version) +
                                    (alreadyInstalled ? " is already installed" : " installed"),
                            Toast.LENGTH_LONG).show();
                    showInstalledPluginsDialog();
                });
            } catch (Throwable error) {
                Log.e("RackForge", "Portable plugin installation failed", error);
                runOnUiThread(() -> new AlertDialog.Builder(this)
                        .setTitle("Plugin was not installed")
                        .setMessage(error.getMessage() == null ? error.toString() : error.getMessage())
                        .setPositiveButton("Close", null)
                        .show());
            } finally {
                if (temporary != null && temporary.isFile() && !temporary.delete()) {
                    Log.w("RackForge", "Could not remove temporary plugin import " + temporary);
                }
            }
        }, "rackforge-plugin-install").start();
    }

    private String selectedFileName(Uri uri) {
        try (Cursor cursor = getContentResolver().query(
                uri, new String[] {OpenableColumns.DISPLAY_NAME}, null, null, null)) {
            if (cursor != null && cursor.moveToFirst()) return cursor.getString(0);
        } catch (Exception error) {
            Log.w("RackForge", "Could not read selected plugin name", error);
        }
        return uri.getLastPathSegment();
    }

    private File copyPluginToPrivateCache(Uri uri) throws Exception {
        File temporary = File.createTempFile("rackforge-import-", ".rfplugin", getCacheDir());
        long total = 0;
        try (InputStream input = getContentResolver().openInputStream(uri);
             FileOutputStream output = new FileOutputStream(temporary)) {
            if (input == null) throw new IllegalStateException("The selected file cannot be opened");
            byte[] buffer = new byte[64 * 1024];
            int read;
            while ((read = input.read(buffer)) >= 0) {
                total += read;
                if (total > MAX_PLUGIN_BYTES) {
                    throw new IllegalStateException("The plugin exceeds the 512 MB package limit");
                }
                output.write(buffer, 0, read);
            }
            if (total == 0) throw new IllegalStateException("The selected plugin is empty");
            output.flush();
            output.getFD().sync();
        } catch (Throwable error) {
            if (temporary.isFile() && !temporary.delete()) temporary.deleteOnExit();
            throw error;
        }
        return temporary;
    }

    private void showInstalledPluginsDialog() {
        try {
            if (installedPluginsDialog != null && installedPluginsDialog.isShowing()) {
                installedPluginsDialog.dismiss();
            }
            JSONObject catalog = new JSONObject(installedPlugins(pluginStoreRoot().getAbsolutePath()));
            JSONArray plugins = catalog.getJSONArray("plugins");
            ScrollView scroll = new ScrollView(this);
            LinearLayout content = new LinearLayout(this);
            content.setOrientation(LinearLayout.VERTICAL);
            content.setPadding(dp(20), dp(22), dp(20), dp(26));
            scroll.addView(content);

            TextView title = new TextView(this);
            title.setText("Installed plugins");
            title.setTextColor(0xFFF2FAFC);
            title.setTextSize(27);
            title.setTypeface(null, android.graphics.Typeface.BOLD);
            content.addView(title);
            TextView subtitle = new TextView(this);
            subtitle.setText("Portable packages are immutable by plugin ID and version");
            subtitle.setTextColor(0xFF91A9B1);
            subtitle.setPadding(0, dp(2), 0, dp(14));
            content.addView(subtitle);

            Button install = toolbarButton("＋  Install .rfplugin", view -> {
                if (installedPluginsDialog != null) installedPluginsDialog.dismiss();
                choosePluginFile();
            });
            install.setTextColor(0xFF5CE2F5);
            LinearLayout.LayoutParams installParams = new LinearLayout.LayoutParams(
                    ViewGroup.LayoutParams.MATCH_PARENT, dp(48));
            installParams.bottomMargin = dp(14);
            content.addView(install, installParams);

            if (plugins.length() == 0) {
                content.addView(settingsValue("Status", "No portable plugins installed"));
            }
            for (int index = 0; index < plugins.length(); index++) {
                JSONObject plugin = plugins.getJSONObject(index);
                content.addView(installedPluginCard(plugin));
            }
            JSONArray warnings = catalog.optJSONArray("warnings");
            if (warnings != null && warnings.length() > 0) {
                TextView warning = settingsHeading("Store warnings");
                content.addView(warning);
                for (int index = 0; index < warnings.length(); index++) {
                    content.addView(settingsValue("Package", warnings.getString(index)));
                }
            }

            AlertDialog dialog = new AlertDialog.Builder(this)
                    .setView(scroll)
                    .setPositiveButton("Close", null)
                    .create();
            installedPluginsDialog = dialog;
            dialog.setOnDismissListener(unused -> {
                if (installedPluginsDialog == dialog) installedPluginsDialog = null;
            });
            dialog.setOnShowListener(unused -> styleFullHeightDialog(dialog));
            dialog.show();
        } catch (Throwable error) {
            Log.e("RackForge", "Could not list installed plugins", error);
            Toast.makeText(this, error.getMessage(), Toast.LENGTH_LONG).show();
        }
    }

    private View installedPluginCard(JSONObject plugin) throws Exception {
        LinearLayout card = settingsCard();
        TextView name = settingsHeading(plugin.getString("plugin_name"));
        card.addView(name);
        String latestVersion = plugin.getString("version");
        card.addView(settingsValue("Latest version", versionLabel(latestVersion)));
        card.addView(settingsValue("Package", plugin.getString("plugin_id")));
        boolean active = plugin.optBoolean("active");
        boolean compatible = plugin.optBoolean("compatible");
        String activeVersion = plugin.isNull("active_version")
                ? null : plugin.optString("active_version", null);
        JSONArray installed = plugin.optJSONArray("installed_versions");
        if (installed != null && installed.length() > 1) {
            List<String> versionNames = new ArrayList<>();
            for (int index = 0; index < installed.length(); index++) {
                versionNames.add(versionLabel(installed.getString(index)));
            }
            card.addView(settingsValue("Installed versions", String.join(" · ", versionNames)));
        }
        String status = active ? "Active"
                : activeVersion != null ? "Update available · active " + versionLabel(activeVersion)
                : compatible ? "Ready" : "Installed · incompatible";
        card.addView(settingsValue("Status", status));
        if (!compatible) {
            card.addView(settingsValue("Reason", plugin.optString("incompatibility", "Unsupported plugin")));
        }
        Button activate = button(active ? "Open in Play" : activeVersion != null ? "Activate latest" : "Activate");
        activate.setEnabled(compatible);
        String root = plugin.getString("package_root");
        String pluginName = plugin.getString("plugin_name");
        String version = plugin.getString("version");
        activate.setOnClickListener(view -> {
            if (installedPluginsDialog != null) installedPluginsDialog.dismiss();
            if (active) showPlay();
            else activatePlugin(root, pluginName, version);
        });
        card.addView(activate, new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT));
        return card;
    }

    private void styleFullHeightDialog(AlertDialog dialog) {
        Window window = dialog.getWindow();
        if (window == null) return;
        window.setBackgroundDrawable(surface(0xFF0A1B24, 24, 0xFF2A4B57, 1));
        window.setDimAmount(0.72f);
        window.addFlags(android.view.WindowManager.LayoutParams.FLAG_DIM_BEHIND);
        window.setGravity(android.view.Gravity.BOTTOM);
        window.setLayout(ViewGroup.LayoutParams.MATCH_PARENT,
                (int) (getResources().getDisplayMetrics().heightPixels * 0.92f));
        dialog.getButton(AlertDialog.BUTTON_POSITIVE).setTextColor(0xFF5CE2F5);
    }

    private File pluginStoreRoot() {
        return new File(getFilesDir(), "plugin-store");
    }

    private File pluginDataRoot() {
        File external = getExternalFilesDir(null);
        if (external == null) throw new IllegalStateException("External app storage unavailable");
        File data = new File(external, "data");
        if (!data.mkdirs() && !data.isDirectory()) {
            throw new IllegalStateException("Cannot create plugin data root");
        }
        return data;
    }

    private void refreshActivePluginMetadata() throws Exception {
        pluginPackageRoot = new File(pluginPackageRoot());
        pluginWebEntry = pluginWebEntry();
        JSONObject context = new JSONObject(pluginWebContext());
        JSONObject instance = context.getJSONObject("instance");
        activePluginName = instance.getString("plugin_name");
        activePluginVersion = instance.optString("plugin_version", "");
    }

    private void activatePlugin(String root, String name, String version) {
        if (engineStarting) {
            Toast.makeText(this, "RackForge is already changing plugins", Toast.LENGTH_SHORT).show();
            return;
        }
        engineStarting = true;
        currentPage = "play";
        showEngineState("Loading " + name,
                "Activating portable plugin " + versionLabel(version) + "…");
        new Thread(() -> {
            File previousRoot = pluginPackageRoot;
            String previousEntry = pluginWebEntry;
            String previousName = activePluginName;
            String previousVersion = activePluginVersion;
            boolean restored = false;
            try {
                releaseMidiNotes();
                stopNativeAudio();
                audioRunning = false;
                if (!activateInstalledPlugin(root, pluginStoreRoot().getAbsolutePath(),
                        pluginDataRoot().getAbsolutePath())) {
                    throw new IllegalStateException("The runtime rejected the selected plugin");
                }
                refreshActivePluginMetadata();
                if (!keyLabSyncPlugins(pluginStoreRoot().getAbsolutePath())) {
                    throw new IllegalStateException("The controller plugin catalog could not be synchronized");
                }
                startAudio();
                refreshKeyLabDisplay();
                preferences.edit().putString("plugin.active_root", pluginPackageRoot.getAbsolutePath()).apply();
                runOnUiThread(() -> {
                    engineStarting = false;
                    if (activePluginLabel != null) activePluginLabel.setText(activePluginDisplayName());
                    if ("diagnostics".equals(currentPage)) renderDiagnostics();
                    else if ("live".equals(currentPage)) showLive();
                    else showPlay();
                    Toast.makeText(this, activePluginDisplayName() + " active",
                            Toast.LENGTH_LONG).show();
                });
                return;
            } catch (Throwable error) {
                Log.e("RackForge", "Plugin activation failed", error);
                if (previousRoot != null) {
                    try {
                        if (!activateInstalledPlugin(previousRoot.getAbsolutePath(),
                                pluginStoreRoot().getAbsolutePath(), pluginDataRoot().getAbsolutePath())) {
                            throw new IllegalStateException("The previous plugin could not be restored");
                        }
                        pluginPackageRoot = previousRoot;
                        pluginWebEntry = previousEntry;
                        activePluginName = previousName;
                        activePluginVersion = previousVersion;
                        keyLabSyncPlugins(pluginStoreRoot().getAbsolutePath());
                        startAudio();
                        refreshKeyLabDisplay();
                        restored = true;
                    } catch (Throwable rollbackError) {
                        error.addSuppressed(rollbackError);
                        Log.e("RackForge", "Plugin activation rollback failed", rollbackError);
                    }
                }
                boolean finalRestored = restored;
                runOnUiThread(() -> {
                    engineStarting = false;
                    if (activePluginLabel != null) activePluginLabel.setText(activePluginDisplayName());
                    if (finalRestored) showPlay();
                    else showEngineState("Plugin activation failed", "The audio engine must be restarted");
                    new AlertDialog.Builder(this)
                            .setTitle("Could not activate " + name)
                            .setMessage(error.getMessage() == null ? error.toString() : error.getMessage())
                            .setPositiveButton("Close", null)
                            .show();
                });
            }
        }, "rackforge-plugin-activate").start();
    }

    private void showViewMenu(View anchor) {
        PopupMenu menu = new PopupMenu(this, anchor);
        menu.getMenu().add("Play").setOnMenuItemClickListener(item -> { showPlay(); return true; });
        menu.getMenu().add("Live").setOnMenuItemClickListener(item -> { showLive(); return true; });
        menu.getMenu().add("Audio & MIDI Settings").setOnMenuItemClickListener(item -> { showSettingsDialog(); return true; });
        menu.getMenu().add("Diagnostics").setOnMenuItemClickListener(item -> { showDiagnostics(); return true; });
        menu.getMenu().add("Reload UI").setOnMenuItemClickListener(item -> {
            if ("diagnostics".equals(currentPage)) renderDiagnostics();
            else if ("live".equals(currentPage)) showLive();
            else showPlay();
            return true;
        });
        menu.show();
    }

    private void showHelpMenu(View anchor) {
        PopupMenu menu = new PopupMenu(this, anchor);
        menu.getMenu().add("About RackForge").setOnMenuItemClickListener(item -> { showAbout(); return true; });
        menu.show();
    }

    private void showAbout() {
        new AlertDialog.Builder(this)
                .setTitle("About RackForge")
                .setMessage("RackForge Android 0.1.0 prototype\nPortable RF-XP10 0.1.3\nRust + Wasmtime + AAudio")
                .setPositiveButton("Close", null)
                .show();
    }

    private void showPlay() {
        currentPage = "play";
        updateModeButtons();
        if (audioRunning && pluginWebEntry != null) {
            webView.loadUrl("https://rackforge.local/plugin/" + pluginWebEntry);
            return;
        }
        showEngineState("Loading plugin", "Validating the portable package and starting AAudio…");
    }

    private void showIdle() {
        currentPage = "idle";
        updateModeButtons();
        showEngineState("RackForge idle",
                "No plugin is active. Choose PLAY and select a plugin to start audio again.");
    }

    private void showLive() {
        currentPage = "live";
        updateModeButtons();
        int midiPorts;
        synchronized (openMidiPorts) { midiPorts = openMidiPorts.size(); }
        String body = "<!doctype html><html><head><meta name='viewport' content='width=device-width,initial-scale=1'>"
                + "<style>" + css() + "</style></head><body><main>"
                + "<div class='eyebrow'>LIVE MODE</div><h1>Performance rack</h1>"
                + "<p class='lead'>A stable performance view that stays separate from the plugin editor in Play.</p>"
                + card("Rack 1", row("Instrument", activePluginDisplayName())
                        + row("Audio", audioRunning ? selectedAudioOutputLabel() : "Starting")
                        + row("MIDI inputs", Integer.toString(midiPorts)))
                + card("System health", row("Thermal", thermalLabel(thermalStatus))
                        + row("Background audio", audioRunning ? "Protected by foreground service" : "Inactive")
                        + "<div class='ok'>Use PLAY to edit sounds; LIVE keeps the performance overview stable.</div>")
                + "</main></body></html>";
        webView.loadDataWithBaseURL("https://rackforge.local/live/", body, "text/html", "UTF-8", null);
    }

    private void showEngineState(String title, String detail) {
        String body = "<!doctype html><html><head><meta name='viewport' content='width=device-width,initial-scale=1'>"
                + "<style>" + css() + "</style></head><body><main>"
                + "<div class='eyebrow'>RACKFORGE ANDROID</div><h1>" + escape(title) + "</h1>"
                + "<p class='lead'>" + escape(detail) + "</p>"
                + card("Engine", "<div class='ok'>Automatic startup</div>"
                        + row("Audio output", selectedAudioOutputLabel())
                        + row("Latency mode", latencyLabel(latencyMode)))
                + "</main></body></html>";
        webView.loadDataWithBaseURL("https://rackforge.local/status/", body, "text/html", "UTF-8", null);
    }

    private void showDiagnostics() {
        currentPage = "diagnostics";
        updateModeButtons();
        renderDiagnostics();
    }

    private void showSettingsDialog() {
        ScrollView scroll = new ScrollView(this);
        scroll.setFillViewport(true);
        LinearLayout content = new LinearLayout(this);
        content.setOrientation(LinearLayout.VERTICAL);
        content.setPadding(dp(20), dp(22), dp(20), dp(28));
        scroll.addView(content);

        TextView title = new TextView(this);
        title.setText("Audio & MIDI");
        title.setTextColor(0xFFF2FAFC);
        title.setTextSize(27);
        title.setTypeface(null, android.graphics.Typeface.BOLD);
        content.addView(title);
        TextView subtitle = new TextView(this);
        subtitle.setText("Configure the real-time engine and connected hardware");
        subtitle.setTextColor(0xFF91A9B1);
        subtitle.setTextSize(13);
        subtitle.setPadding(0, dp(2), 0, dp(14));
        content.addView(subtitle);

        Button refreshDevices = toolbarButton("↻  Refresh devices", view -> {});
        refreshDevices.setTextColor(0xFF5CE2F5);
        LinearLayout.LayoutParams refreshParams = new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, dp(48));
        refreshParams.bottomMargin = dp(14);
        content.addView(refreshDevices, refreshParams);

        LinearLayout audioCard = settingsCard();
        content.addView(audioCard);
        audioCard.addView(settingsHeading("Audio output"));
        audioCard.addView(settingsValue("Driver", "AAudio · native callback"));

        Spinner output = new Spinner(this);
        ArrayAdapter<AudioOutputChoice> outputs = new ArrayAdapter<>(this,
                android.R.layout.simple_spinner_item, new ArrayList<>(audioOutputChoices));
        outputs.setDropDownViewResource(android.R.layout.simple_spinner_dropdown_item);
        output.setAdapter(outputs);
        int outputIndex = 0;
        for (int index = 0; index < audioOutputChoices.size(); index++) {
            if (audioOutputChoices.get(index).key.equals(selectedAudioDeviceKey)) outputIndex = index;
        }
        output.setSelection(outputIndex);
        audioCard.addView(settingsControl("Output device", output));

        Spinner latency = new Spinner(this);
        latency.setAdapter(new ArrayAdapter<>(this, android.R.layout.simple_spinner_dropdown_item,
                new String[] {"Low latency", "Balanced", "Safe"}));
        latency.setSelection(latencyMode);
        audioCard.addView(settingsControl("Latency mode", latency));
        try {
            JSONObject status = new JSONObject(nativeAudioStatus());
            int actualRate = status.optInt("sample_rate", SAMPLE_RATE);
            int burst = status.optInt("frames_per_burst", 0);
            int buffer = status.optInt("buffer_size_frames", 0);
            int xruns = status.optInt("xruns", 0);
            double averageUs = status.optDouble("average_callback_us", 0.0);
            double maximumUs = status.optDouble("maximum_callback_us", 0.0);
            double load = status.optDouble("callback_load_percent", 0.0);
            long droppedMidi = status.optLong("midi_dropped_events", 0);
            audioCard.addView(settingsValue("Stream", actualRate + " Hz · " + burst + " frames/burst"));
            audioCard.addView(settingsValue("Buffer", buffer + " frames · " + xruns + " xruns"));
            audioCard.addView(settingsValue("Callback CPU", String.format(Locale.ROOT,
                    "%.1f%% · avg %.0f µs · max %.0f µs", load, averageUs, maximumUs)));
            audioCard.addView(settingsValue("MIDI queue", droppedMidi == 0
                    ? "No dropped events" : droppedMidi + " dropped events"));
        } catch (Exception ignored) {
            audioCard.addView(settingsValue("Stream", SAMPLE_RATE + " Hz · starting"));
        }

        Spinner gain = new Spinner(this);
        String[] gains = {"+0 dB", "+3 dB", "+6 dB", "+9 dB", "+12 dB"};
        gain.setAdapter(new ArrayAdapter<>(this, android.R.layout.simple_spinner_dropdown_item, gains));
        gain.setSelection(Math.max(0, Math.min(4, outputGainDb / 3)));
        audioCard.addView(settingsControl("Output gain", gain));

        LinearLayout midiCard = settingsCard();
        content.addView(midiCard);
        midiCard.addView(settingsHeading("MIDI inputs"));
        MidiManager midiManager = (MidiManager) getSystemService(Context.MIDI_SERVICE);
        MidiDeviceInfo[] midiDevices = midiManager == null ? new MidiDeviceInfo[0] : midiManager.getDevices();
        Set<String> savedMidi = preferences.getStringSet("midi.inputs", null);
        Map<CheckBox, String> midiChecks = new LinkedHashMap<>();
        for (MidiDeviceInfo info : midiDevices) {
            String name = midiDeviceName(info);
            CheckBox check = new CheckBox(this);
            check.setText(name);
            check.setTextColor(0xFFE2F2F5);
            check.setChecked(savedMidi == null || savedMidi.contains(name));
            midiCard.addView(check);
            midiChecks.put(check, name);
        }
        if (midiDevices.length == 0) midiCard.addView(settingsValue("Status", "No MIDI devices connected"));

        Button test = button("Test C4");
        test.setEnabled(audioRunning);
        test.setOnClickListener(view -> playTestNote());
        midiCard.addView(test);

        LinearLayout httpCard = settingsCard();
        content.addView(httpCard);
        httpCard.addView(settingsHeading("HTTP server"));
        Switch http = new Switch(this);
        http.setText("Enable HTTP server");
        http.setTextColor(0xFFE2F2F5);
        http.setChecked(false);
        http.setEnabled(false);
        httpCard.addView(http);
        httpCard.addView(settingsValue("Status", "Disabled by default · Android integration pending"));

        AlertDialog dialog = new AlertDialog.Builder(this)
                .setView(scroll)
                .setNegativeButton("Discard", null)
                .setPositiveButton("Apply", (unused, which) -> {
                    AudioOutputChoice choice = audioOutputChoices.get(output.getSelectedItemPosition());
                    selectedAudioDeviceId = choice.id;
                    selectedAudioDeviceKey = choice.key;
                    latencyMode = latency.getSelectedItemPosition();
                    outputGainDb = gain.getSelectedItemPosition() * 3;
                    java.util.HashSet<String> enabledMidi = new java.util.HashSet<>();
                    for (Map.Entry<CheckBox, String> entry : midiChecks.entrySet()) {
                        if (entry.getKey().isChecked()) enabledMidi.add(entry.getValue());
                    }
                    preferences.edit()
                            .putString("audio.output", selectedAudioDeviceKey)
                            .putInt("audio.latency", latencyMode)
                            .putInt("audio.gain_db", outputGainDb)
                            .putStringSet("midi.inputs", enabledMidi)
                            .apply();
                    setNativeOutputGain(outputGainDb);
                    if (audioRunning) switchAudioOutput();
                    closeMidi();
                    if (audioRunning) openMidiInputs();
                    if ("diagnostics".equals(currentPage)) renderDiagnostics();
                    else if ("live".equals(currentPage)) showLive();
                    else showPlay();
                })
                .create();
        refreshDevices.setOnClickListener(view -> {
            refreshAudioOutputs();
            dialog.dismiss();
            showSettingsDialog();
            Toast.makeText(this, "Audio and MIDI devices refreshed", Toast.LENGTH_SHORT).show();
        });
        dialog.setOnShowListener(unused -> {
            Window window = dialog.getWindow();
            if (window != null) {
                window.setBackgroundDrawable(surface(0xFF0A1B24, 24, 0xFF2A4B57, 1));
                window.setDimAmount(0.72f);
                window.addFlags(android.view.WindowManager.LayoutParams.FLAG_DIM_BEHIND);
                window.setGravity(android.view.Gravity.BOTTOM);
                window.setLayout(ViewGroup.LayoutParams.MATCH_PARENT,
                        (int) (getResources().getDisplayMetrics().heightPixels * 0.92f));
            }
            dialog.getButton(AlertDialog.BUTTON_POSITIVE).setTextColor(0xFF5CE2F5);
            dialog.getButton(AlertDialog.BUTTON_NEGATIVE).setTextColor(0xFF91A9B1);
        });
        dialog.show();
    }

    private LinearLayout settingsCard() {
        LinearLayout card = new LinearLayout(this);
        card.setOrientation(LinearLayout.VERTICAL);
        card.setPadding(dp(16), dp(4), dp(16), dp(14));
        card.setBackground(surface(0xFF112B36, 16, 0xFF2A4B57, 1));
        LinearLayout.LayoutParams params = new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT);
        params.bottomMargin = dp(12);
        card.setLayoutParams(params);
        return card;
    }

    private TextView settingsHeading(String text) {
        TextView view = new TextView(this);
        view.setText(text);
        view.setTextColor(0xFF5CE2F5);
        view.setTextSize(19);
        view.setTypeface(null, android.graphics.Typeface.BOLD);
        view.setPadding(0, dp(16), 0, dp(8));
        return view;
    }

    private LinearLayout settingsValue(String label, String value) {
        TextView text = new TextView(this);
        text.setText(value);
        text.setTextColor(0xFFE2F2F5);
        return settingsControl(label, text);
    }

    private LinearLayout settingsControl(String label, View control) {
        LinearLayout row = new LinearLayout(this);
        row.setGravity(android.view.Gravity.CENTER_VERTICAL);
        row.setPadding(0, dp(6), 0, dp(6));
        TextView name = new TextView(this);
        name.setText(label);
        name.setTextColor(0xFF91A9B1);
        row.addView(name, new LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 0.42f));
        row.addView(control, new LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 0.58f));
        return row;
    }

    private static String latencyLabel(int mode) {
        return switch (mode) {
            case 1 -> "Balanced";
            case 2 -> "Safe";
            default -> "Low latency";
        };
    }

    private String activePluginDisplayName() {
        return activePluginName + (activePluginVersion.isBlank()
                ? "" : " " + versionLabel(activePluginVersion));
    }

    private static String versionLabel(String version) {
        if (version == null || version.isBlank()) return "";
        String clean = version.trim();
        if (clean.startsWith("v") || clean.startsWith("V")) clean = clean.substring(1);
        return "v" + clean;
    }

    private static String thermalLabel(int status) {
        return switch (status) {
            case PowerManager.THERMAL_STATUS_LIGHT -> "Light";
            case PowerManager.THERMAL_STATUS_MODERATE -> "Moderate";
            case PowerManager.THERMAL_STATUS_SEVERE -> "Severe · reduce workload";
            case PowerManager.THERMAL_STATUS_CRITICAL -> "Critical";
            case PowerManager.THERMAL_STATUS_EMERGENCY -> "Emergency";
            case PowerManager.THERMAL_STATUS_SHUTDOWN -> "Shutdown imminent";
            default -> "Nominal";
        };
    }

    private int dp(int value) {
        return Math.round(value * getResources().getDisplayMetrics().density);
    }

    private GradientDrawable surface(int color, int radiusDp, int strokeColor, int strokeDp) {
        GradientDrawable drawable = new GradientDrawable();
        drawable.setColor(color);
        drawable.setCornerRadius(dp(radiusDp));
        if (strokeDp > 0) drawable.setStroke(dp(strokeDp), strokeColor);
        return drawable;
    }

    private Button button(String text) {
        Button button = new Button(this);
        button.setText(text);
        button.setTextColor(0xFF07141A);
        button.setBackgroundTintList(android.content.res.ColorStateList.valueOf(0xFF5CE2F5));
        button.setAllCaps(false);
        return button;
    }

    private void startEngine() {
        if (engineStarting || audioRunning) return;
        engineStarting = true;
        Thread loader = new Thread(() -> {
            try {
                byte[] archive = readAsset("plugins/rf-xp10.rfplugin");
                File store = pluginStoreRoot();
                File data = pluginDataRoot();
                File legacyAddons = new File(data, "addons");
                if (!store.mkdirs() && !store.isDirectory()) throw new IllegalStateException("Cannot create plugin store");
                // Android's scoped external storage may report EACCES instead of ENOENT
                // when the core probes this legacy migration directory.
                if (!legacyAddons.mkdirs() && !legacyAddons.isDirectory()) {
                    throw new IllegalStateException("Cannot create legacy plugin migration directory");
                }
                if (!initializeEngine(archive, store.getAbsolutePath(), data.getAbsolutePath())) {
                    throw new IllegalStateException("Native engine rejected initialization");
                }
                String preferredRoot = preferences.getString("plugin.active_root", null);
                if (preferredRoot != null && !preferredRoot.equals(pluginPackageRoot())) {
                    try {
                        if (!activateInstalledPlugin(preferredRoot, store.getAbsolutePath(), data.getAbsolutePath())) {
                            throw new IllegalStateException("The saved plugin could not be activated");
                        }
                    } catch (Throwable restoreError) {
                        Log.w("RackForge", "Saved plugin is unavailable; using bundled RF-XP10", restoreError);
                        preferences.edit().remove("plugin.active_root").apply();
                    }
                }
                refreshActivePluginMetadata();
                if (!keyLabSyncPlugins(store.getAbsolutePath())) {
                    throw new IllegalStateException("The controller plugin catalog could not be synchronized");
                }
                startAudio();
                openMidiInputs();
                runOnUiThread(() -> {
                    engineStarting = false;
                    if (activePluginLabel != null) activePluginLabel.setText(activePluginDisplayName());
                    if ("live".equals(currentPage)) showLive(); else showPlay();
                    Toast.makeText(this, activePluginName + " ready · 48 kHz", Toast.LENGTH_SHORT).show();
                });
            } catch (Throwable error) {
                Log.e("RackForge", "Plugin engine initialization failed", error);
                runOnUiThread(() -> {
                    engineStarting = false;
                    showEngineState("Plugin could not start",
                            error.getMessage() == null ? "Unknown engine error" : error.getMessage());
                    Toast.makeText(this, error.getMessage(), Toast.LENGTH_LONG).show();
                });
            }
        }, "rackforge-engine-loader");
        loader.start();
    }

    private byte[] readAsset(String path) throws Exception {
        try (InputStream input = getAssets().open(path);
             ByteArrayOutputStream output = new ByteArrayOutputStream()) {
            byte[] buffer = new byte[16 * 1024];
            int read;
            while ((read = input.read(buffer)) >= 0) output.write(buffer, 0, read);
            return output.toByteArray();
        }
    }

    private void startAudio() {
        if (!startNativeAudio(selectedAudioDeviceId, latencyMode)) {
            throw new IllegalStateException("Native low-latency audio rejected the selected output");
        }
        audioRunning = true;
        startAudioService();
    }

    private void startAudioService() {
        Intent service = new Intent(this, AudioEngineService.class);
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) startForegroundService(service);
        else startService(service);
    }

    private void recoverAudioStream(int errorCode) {
        if (audioRecoveryInProgress || engineStarting) return;
        audioRecoveryInProgress = true;
        audioRunning = false;
        new Thread(() -> {
            try {
                stopNativeAudio();
                if (!startNativeAudio(selectedAudioDeviceId, latencyMode)) {
                    throw new IllegalStateException("AAudio could not reopen after error " + errorCode);
                }
                audioRunning = true;
                runOnUiThread(() -> {
                    audioRecoveryInProgress = false;
                    Toast.makeText(this, "Audio stream reconnected", Toast.LENGTH_SHORT).show();
                    if ("live".equals(currentPage)) showLive();
                });
            } catch (Throwable error) {
                Log.e("RackForge", "AAudio recovery failed", error);
                runOnUiThread(() -> {
                    audioRecoveryInProgress = false;
                    Toast.makeText(this, "Audio disconnected · open Settings to retry", Toast.LENGTH_LONG).show();
                });
            }
        }, "rackforge-audio-recovery").start();
    }

    private void switchAudioOutput() {
        int deviceId = selectedAudioDeviceId;
        String label = selectedAudioOutputLabel();
        new Thread(() -> {
            try {
                if (!startNativeAudio(deviceId, latencyMode)) throw new IllegalStateException("Cannot open " + label);
                runOnUiThread(() -> Toast.makeText(this, "Audio output: " + label, Toast.LENGTH_SHORT).show());
            } catch (Throwable error) {
                Log.e("RackForge", "Audio output switch failed", error);
                runOnUiThread(() -> Toast.makeText(this, error.getMessage(), Toast.LENGTH_LONG).show());
            }
        }, "rackforge-audio-switch").start();
    }

    private void refreshAudioOutputs() {
        AudioManager manager = (AudioManager) getSystemService(Context.AUDIO_SERVICE);
        int previous = selectedAudioDeviceId;
        audioOutputChoices.clear();
        audioOutputChoices.add(new AudioOutputChoice(0, "default", "System default"));
        AudioDeviceInfo[] outputs = manager.getDevices(AudioManager.GET_DEVICES_OUTPUTS);
        java.util.Arrays.sort(outputs, Comparator.comparingInt(AudioDeviceInfo::getId));
        for (AudioDeviceInfo output : outputs) {
            if (!output.isSink()) continue;
            String product = output.getProductName() == null ? "Audio device" : output.getProductName().toString();
            String key = output.getType() + "|" + product;
            audioOutputChoices.add(new AudioOutputChoice(output.getId(), key,
                    typeLabel(output.getType()) + " · " + product));
        }
        int selectedIndex = 0;
        for (int index = 0; index < audioOutputChoices.size(); index++) {
            AudioOutputChoice choice = audioOutputChoices.get(index);
            if (choice.key.equals(selectedAudioDeviceKey)
                    || ("default".equals(selectedAudioDeviceKey) && choice.id == previous)) {
                selectedIndex = index;
            }
        }
        boolean disappeared = previous != 0 && selectedIndex == 0;
        selectedAudioDeviceId = audioOutputChoices.get(selectedIndex).id;
        refreshingAudioOutputs = true;
        ArrayAdapter<AudioOutputChoice> adapter = new ArrayAdapter<>(this,
                android.R.layout.simple_spinner_item, new ArrayList<>(audioOutputChoices));
        adapter.setDropDownViewResource(android.R.layout.simple_spinner_dropdown_item);
        audioOutputSpinner.setAdapter(adapter);
        audioOutputSpinner.setSelection(selectedIndex, false);
        refreshingAudioOutputs = false;
        if (disappeared && audioRunning) switchAudioOutput();
    }

    private void registerAudioDeviceUpdates() {
        AudioManager manager = (AudioManager) getSystemService(Context.AUDIO_SERVICE);
        audioDeviceCallback = new AudioDeviceCallback() {
            @Override public void onAudioDevicesAdded(AudioDeviceInfo[] addedDevices) {
                runOnUiThread(() -> {
                    refreshAudioOutputs();
                    if ("diagnostics".equals(currentPage)) renderDiagnostics();
                    else if ("live".equals(currentPage)) showLive();
                });
            }

            @Override public void onAudioDevicesRemoved(AudioDeviceInfo[] removedDevices) {
                runOnUiThread(() -> {
                    refreshAudioOutputs();
                    if ("diagnostics".equals(currentPage)) renderDiagnostics();
                    else if ("live".equals(currentPage)) showLive();
                });
            }
        };
        manager.registerAudioDeviceCallback(audioDeviceCallback, null);
    }

    private void registerMidiDeviceUpdates() {
        MidiManager manager = (MidiManager) getSystemService(Context.MIDI_SERVICE);
        if (manager == null) return;
        midiDeviceCallback = new MidiManager.DeviceCallback() {
            @Override public void onDeviceAdded(MidiDeviceInfo device) {
                if (device.getType() == MidiDeviceInfo.TYPE_USB) scheduleMidiReconnect();
            }

            @Override public void onDeviceRemoved(MidiDeviceInfo device) {
                if (device.getType() != MidiDeviceInfo.TYPE_USB) return;
                if (audioRunning) {
                    releaseMidiNotes();
                    Log.i("RackForge", "MIDI device removed; sustain and active notes released");
                    Toast.makeText(MainActivity.this,
                            "MIDI disconnected · notes released", Toast.LENGTH_SHORT).show();
                }
                scheduleMidiReconnect();
            }
        };
        manager.registerDeviceCallback(midiDeviceCallback, mainHandler);
    }

    private void scheduleMidiReconnect() {
        mainHandler.removeCallbacks(midiReconnect);
        mainHandler.postDelayed(midiReconnect, 300);
    }

    private void registerThermalMonitoring() {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.Q) return;
        thermalMonitor = new ThermalMonitor(this);
        thermalMonitor.start();
    }

    private static final class ThermalMonitor {
        private final MainActivity activity;
        private final PowerManager power;
        private final PowerManager.OnThermalStatusChangedListener listener;

        ThermalMonitor(MainActivity activity) {
            this.activity = activity;
            power = (PowerManager) activity.getSystemService(Context.POWER_SERVICE);
            listener = status -> {
                activity.thermalStatus = status;
                if ("live".equals(activity.currentPage)) activity.runOnUiThread(activity::showLive);
            };
        }

        void start() {
            activity.thermalStatus = power.getCurrentThermalStatus();
            power.addThermalStatusListener(listener);
        }

        void stop() {
            power.removeThermalStatusListener(listener);
        }
    }

    private String selectedAudioOutputLabel() {
        for (AudioOutputChoice choice : audioOutputChoices) {
            if (choice.id == selectedAudioDeviceId) return choice.label;
        }
        return "System default";
    }

    private static String typeLabel(int type) {
        return switch (type) {
            case AudioDeviceInfo.TYPE_BUILTIN_SPEAKER -> "Built-in speaker";
            case AudioDeviceInfo.TYPE_BUILTIN_EARPIECE -> "Earpiece";
            case AudioDeviceInfo.TYPE_USB_DEVICE -> "USB audio";
            case AudioDeviceInfo.TYPE_USB_HEADSET -> "USB headset";
            case AudioDeviceInfo.TYPE_USB_ACCESSORY -> "USB accessory";
            case AudioDeviceInfo.TYPE_WIRED_HEADPHONES -> "Headphones";
            case AudioDeviceInfo.TYPE_WIRED_HEADSET -> "Wired headset";
            case AudioDeviceInfo.TYPE_BLUETOOTH_A2DP -> "Bluetooth";
            default -> "Audio output";
        };
    }

    private static final class AudioOutputChoice {
        final int id;
        final String key;
        final String label;

        AudioOutputChoice(int id, String key, String label) {
            this.id = id;
            this.key = key;
            this.label = label;
        }

        @Override public String toString() { return label; }
    }

    private final class MidiStreamDecoder {
        private final boolean keyLabSurface;
        private final boolean forwardMidi;
        private final MidiInputPort keyLabDestination;
        private final int generation;
        private int runningStatus = -1;
        private int messageStatus = -1;
        private int expectedDataBytes;
        private int dataCount;
        private final byte[] messageData = new byte[2];
        private boolean inSysEx;

        MidiStreamDecoder(boolean keyLabSurface, boolean forwardMidi,
                MidiInputPort keyLabDestination, int generation) {
            this.keyLabSurface = keyLabSurface;
            this.forwardMidi = forwardMidi;
            this.keyLabDestination = keyLabDestination;
            this.generation = generation;
        }

        synchronized void accept(byte[] bytes, int offset, int count) {
            int end = Math.min(bytes.length, offset + count);
            for (int index = Math.max(0, offset); index < end; index++) {
                int value = bytes[index] & 0xFF;
                if (value >= 0xF8) continue;
                if ((value & 0x80) != 0) {
                    acceptStatus(value);
                } else {
                    acceptData((byte) value);
                }
            }
        }

        private void acceptStatus(int status) {
            if (status == 0xF0) {
                inSysEx = true;
                resetMessage();
                runningStatus = -1;
                return;
            }
            if (status == 0xF7) {
                inSysEx = false;
                resetMessage();
                runningStatus = -1;
                return;
            }
            if (inSysEx) return;
            if (status >= 0x80 && status <= 0xEF) {
                runningStatus = status;
                messageStatus = status;
                expectedDataBytes = channelDataLength(status);
                dataCount = 0;
            } else {
                runningStatus = -1;
                resetMessage();
            }
        }

        private void acceptData(byte value) {
            if (inSysEx) return;
            if (messageStatus < 0) {
                if (runningStatus < 0) return;
                messageStatus = runningStatus;
                expectedDataBytes = channelDataLength(runningStatus);
                dataCount = 0;
            }
            if (dataCount < messageData.length) messageData[dataCount] = value;
            dataCount++;
            if (dataCount != expectedDataBytes) return;
            int data1 = messageData[0] & 0x7F;
            int data2 = expectedDataBytes > 1 ? messageData[1] & 0x7F : 0;
            boolean consumed = false;
            if (keyLabSurface && (messageStatus & 0xF0) == 0xB0) {
                String response = keyLabHandleMidi(messageStatus, data1, data2);
                if (response != null) {
                    consumed = true;
                    handleKeyLabResponse(response, keyLabDestination, generation);
                    if (data1 >= 44 && data1 <= 47 && data2 == 127) {
                        scheduleKeyLabLongPress(keyLabDestination, generation);
                    }
                }
            }
            if (!consumed && forwardMidi && audioRunning) {
                sendMidiMessage(messageStatus, data1, data2, expectedDataBytes + 1);
            }
            messageStatus = runningStatus;
            dataCount = 0;
        }

        private void resetMessage() {
            messageStatus = -1;
            expectedDataBytes = 0;
            dataCount = 0;
        }

        private static int channelDataLength(int status) {
            int command = status & 0xF0;
            return command == 0xC0 || command == 0xD0 ? 1 : 2;
        }
    }

    private void handleKeyLabResponse(String json, MidiInputPort destination, int generation) {
        try {
            JSONObject response = new JSONObject(json);
            if (destination != null) {
                sendControllerPlan(destination, response.getJSONArray("plan").toString(), generation);
            }
            JSONObject command = response.optJSONObject("command");
            if (command != null) handleKeyLabCommand(command);
            long restoreAfterMs = response.optLong("restore_header_after_ms", 0);
            int headerGeneration = keyLabHeaderGeneration.incrementAndGet();
            if (restoreAfterMs > 0) {
                mainHandler.postDelayed(() -> {
                    if (generation != midiGeneration
                            || headerGeneration != keyLabHeaderGeneration.get()) return;
                    refreshKeyLabDisplay();
                }, restoreAfterMs);
            }
        } catch (Exception error) {
            Log.e("RackForge", "Invalid KeyLab controller response", error);
        }
    }

    private void scheduleKeyLabLongPress(MidiInputPort destination, int generation) {
        mainHandler.postDelayed(() -> {
            if (generation != midiGeneration) return;
            String response = keyLabPollLongPress();
            if (response != null) handleKeyLabResponse(response, destination, generation);
        }, 710);
    }

    private void handleKeyLabCommand(JSONObject command) {
        String type = command.optString("type");
        switch (type) {
            case "set_mode" -> mainHandler.post(() -> showControllerMode(command.optString("mode")));
            case "select_plugin" -> mainHandler.post(() -> activatePlugin(
                    command.optString("root"), command.optString("name"),
                    command.optString("version")));
            case "select_sound" -> selectControllerSound(command.optString("sound_id"));
            case "return_mode" -> {
                String soundId = command.optString("sound_id");
                if (!soundId.isBlank()) selectControllerSound(soundId);
                mainHandler.post(() -> showControllerMode(command.optString("mode")));
            }
            case "force_home" -> mainHandler.post(this::emergencyControllerHome);
            default -> Log.d("RackForge", "Unsupported KeyLab menu command " + type);
        }
    }

    private void showControllerMode(String mode) {
        if ("live".equals(mode)) {
            currentPage = "live";
            showLive();
        } else if ("play".equals(mode)) {
            currentPage = "play";
            showPlay();
        }
    }

    private void emergencyControllerHome() {
        releaseMidiNotes();
        audioRunning = false;
        stopNativeAudio();
        stopService(new Intent(this, AudioEngineService.class));
        currentPage = "idle";
        showIdle();
        Toast.makeText(this, "Emergency HOME · plugin stopped", Toast.LENGTH_SHORT).show();
    }

    private void selectControllerSound(String soundId) {
        if (soundId == null || soundId.isBlank()) return;
        new Thread(() -> {
            if (!selectPluginSound(soundId)) {
                Log.w("RackForge", "KeyLab could not select sound " + soundId);
                return;
            }
            if (!keyLabSyncActivePlugin()) {
                Log.w("RackForge", "KeyLab could not confirm sound " + soundId);
                return;
            }
            refreshKeyLabDisplay();
            runOnUiThread(() -> sendPluginMessage(pluginWebContext()));
        }, "rackforge-keylab-sound").start();
    }

    private void playTestNote() {
        sendMidiMessage(0x90, 60, 100, 3);
        webView.postDelayed(() -> sendMidiMessage(0x80, 60, 0, 3), 400);
    }

    private void openMidiInputs() {
        MidiManager manager = (MidiManager) getSystemService(Context.MIDI_SERVICE);
        if (manager == null) return;
        int generation = midiGeneration;
        Set<String> enabledInputs = preferences.getStringSet("midi.inputs", null);
        for (MidiDeviceInfo info : manager.getDevices()) {
            if (info.getType() != MidiDeviceInfo.TYPE_USB) continue;
            if (enabledInputs != null && !enabledInputs.contains(midiDeviceName(info))) continue;
            manager.openDevice(info, device -> {
                if (device == null) return;
                if (!audioRunning || generation != midiGeneration) {
                    try { device.close(); } catch (Exception ignored) { }
                    return;
                }
                synchronized (openMidiDevices) { openMidiDevices.add(device); }
                boolean keyLab = isKeyLabDevice(info);
                MidiInputPort keyLabDestination = keyLab
                        ? openKeyLabDestination(device, info, generation) : null;
                for (MidiDeviceInfo.PortInfo portInfo : info.getPorts()) {
                    if (portInfo.getType() != MidiDeviceInfo.PortInfo.TYPE_OUTPUT) continue;
                    boolean keyLabPrimary = keyLab && isPrimaryKeyLabPort(portInfo);
                    boolean forwardMidi = !keyLab || keyLabPrimary;
                    MidiOutputPort port = device.openOutputPort(portInfo.getPortNumber());
                    if (port == null) continue;
                    if (!audioRunning || generation != midiGeneration) {
                        try { port.close(); } catch (Exception ignored) { }
                        continue;
                    }
                    if (keyLab) {
                        Log.i("RackForge", "KeyLab source port " + portInfo.getPortNumber()
                                + " " + portInfo.getName() + " primary=" + keyLabPrimary);
                    }
                    port.connect(new MidiReceiver() {
                        private final MidiStreamDecoder decoder = new MidiStreamDecoder(
                                keyLabPrimary, forwardMidi, keyLabDestination, generation);

                        @Override
                        public void onSend(byte[] data, int offset, int count, long timestamp) {
                            decoder.accept(data, offset, count);
                        }
                    });
                    synchronized (openMidiPorts) { openMidiPorts.add(port); }
                }
            }, null);
        }
    }

    private MidiInputPort openKeyLabDestination(MidiDevice device, MidiDeviceInfo info,
            int generation) {
        MidiDeviceInfo.PortInfo selected = null;
        MidiDeviceInfo.PortInfo fallback = null;
        for (MidiDeviceInfo.PortInfo portInfo : info.getPorts()) {
            if (portInfo.getType() != MidiDeviceInfo.PortInfo.TYPE_INPUT) continue;
            if (fallback == null) fallback = portInfo;
            String name = portInfo.getName();
            String folded = name == null ? "" : name.toLowerCase(Locale.ROOT);
            if (folded.contains("mcu") || folded.contains("hui")
                    || folded.contains("dinthru") || folded.contains("alv")) continue;
            if (folded.contains("midi") || folded.contains("keylab")
                    || folded.contains("kl essential")) {
                selected = portInfo;
                break;
            }
        }
        if (selected == null) selected = fallback;
        if (selected == null) {
            Log.w("RackForge", "KeyLab detected without a MIDI destination port");
            return null;
        }
        MidiInputPort port = device.openInputPort(selected.getPortNumber());
        if (port == null) {
            Log.w("RackForge", "Could not open the KeyLab MIDI destination");
            return null;
        }
        if (!audioRunning || generation != midiGeneration) {
            try { port.close(); } catch (Exception ignored) { }
            return null;
        }
        synchronized (openMidiDestinations) { openMidiDestinations.add(port); }
        synchronized (openKeyLabDestinations) { openKeyLabDestinations.add(port); }
        sendControllerPlan(port, keyLabAcquirePlan(), generation);
        Log.i("RackForge", "KeyLab controller runtime acquired on Android");
        return port;
    }

    private void sendControllerPlan(MidiInputPort port, String json, int generation) {
        try {
            JSONArray plan = new JSONArray(json);
            long delayMs = 0;
            for (int index = 0; index < plan.length(); index++) {
                JSONObject step = plan.getJSONObject(index);
                JSONArray values = step.getJSONArray("bytes");
                byte[] message = new byte[values.length()];
                for (int byteIndex = 0; byteIndex < values.length(); byteIndex++) {
                    message[byteIndex] = (byte) values.getInt(byteIndex);
                }
                long scheduledAt = delayMs;
                mainHandler.postDelayed(() -> {
                    if (generation != midiGeneration) return;
                    synchronized (openMidiDestinations) {
                        if (!openMidiDestinations.contains(port)) return;
                    }
                    try {
                        port.send(message, 0, message.length);
                    } catch (Exception error) {
                        Log.e("RackForge", "KeyLab MIDI output failed", error);
                    }
                }, scheduledAt);
                delayMs += step.optLong("settle_after_ms", 0);
            }
        } catch (Exception error) {
            Log.e("RackForge", "Invalid KeyLab controller plan", error);
        }
    }

    private void refreshKeyLabDisplay() {
        String plan = keyLabRenderPlan();
        if (plan == null) return;
        int generation = midiGeneration;
        synchronized (openKeyLabDestinations) {
            for (MidiInputPort port : openKeyLabDestinations) {
                sendControllerPlan(port, plan, generation);
            }
        }
    }

    private static void sendControllerPlanImmediately(MidiInputPort port, String json) {
        try {
            JSONArray plan = new JSONArray(json);
            for (int index = 0; index < plan.length(); index++) {
                JSONArray values = plan.getJSONObject(index).getJSONArray("bytes");
                byte[] message = new byte[values.length()];
                for (int byteIndex = 0; byteIndex < values.length(); byteIndex++) {
                    message[byteIndex] = (byte) values.getInt(byteIndex);
                }
                port.send(message, 0, message.length);
            }
        } catch (Exception error) {
            Log.w("RackForge", "KeyLab restore was incomplete", error);
        }
    }

    private static boolean isKeyLabDevice(MidiDeviceInfo info) {
        String folded = midiDeviceName(info).toLowerCase(Locale.ROOT);
        return folded.contains("keylab essential") || folded.contains("kl essential");
    }

    private static boolean isPrimaryKeyLabPort(MidiDeviceInfo.PortInfo portInfo) {
        String name = portInfo.getName();
        String folded = name == null ? "" : name.toLowerCase(Locale.ROOT);
        if (folded.contains("mcu") || folded.contains("hui")
                || folded.contains("dinthru") || folded.contains("alv")) return false;
        return portInfo.getPortNumber() == 0;
    }

    private static String midiDeviceName(MidiDeviceInfo info) {
        Bundle properties = info.getProperties();
        String name = properties.getString(MidiDeviceInfo.PROPERTY_PRODUCT);
        if (name == null || name.isBlank()) {
            name = properties.getString(MidiDeviceInfo.PROPERTY_NAME, "MIDI device");
        }
        return name;
    }

    private void closeMidi() {
        synchronized (openKeyLabDestinations) {
            for (MidiInputPort port : openKeyLabDestinations) {
                sendControllerPlanImmediately(port, keyLabRestorePlan());
            }
            openKeyLabDestinations.clear();
        }
        midiGeneration++;
        synchronized (openMidiPorts) {
            for (MidiOutputPort port : openMidiPorts) try { port.close(); } catch (Exception ignored) { }
            openMidiPorts.clear();
        }
        synchronized (openMidiDestinations) {
            for (MidiInputPort port : openMidiDestinations) {
                try { port.close(); } catch (Exception ignored) { }
            }
            openMidiDestinations.clear();
        }
        synchronized (openMidiDevices) {
            for (MidiDevice device : openMidiDevices) try { device.close(); } catch (Exception ignored) { }
            openMidiDevices.clear();
        }
    }

    private void renderDiagnostics() {
        String html = "<!doctype html><html><head><meta name='viewport' content='width=device-width,initial-scale=1'>"
                + "<style>" + css() + "</style></head><body><main>"
                + "<div class='eyebrow'>RACKFORGE ANDROID</div>"
                + "<h1>USB host prototype</h1>"
                + "<p class='lead'>Connect the powered hub, MIDI controller and USB audio interface, then return to this screen.</p>"
                + featureCard()
                + pluginCard()
                + usbCard()
                + midiCard()
                + audioCard()
                + "<p class='foot'>Start the RF-XP10 engine below, test C4, then play the connected USB MIDI controller.</p>"
                + "</main></body></html>";
        webView.loadDataWithBaseURL("https://rackforge.local/", html, "text/html", "UTF-8", null);
    }

    private String featureCard() {
        PackageManager packages = getPackageManager();
        return card("Android capabilities",
                row("USB host", supported(packages, PackageManager.FEATURE_USB_HOST))
                        + row("MIDI", supported(packages, PackageManager.FEATURE_MIDI))
                        + row("Low-latency audio", supported(packages, PackageManager.FEATURE_AUDIO_LOW_LATENCY))
                        + row("Professional audio", supported(packages, PackageManager.FEATURE_AUDIO_PRO)));
    }

    private String pluginCard() {
        try (InputStream input = getAssets().open("plugins/rf-xp10.rfplugin")) {
            MessageDigest digest = MessageDigest.getInstance("SHA-256");
            byte[] buffer = new byte[16 * 1024];
            long bytes = 0;
            int read;
            while ((read = input.read(buffer)) >= 0) {
                digest.update(buffer, 0, read);
                bytes += read;
            }
            return card("Portable plugin",
                    row("Package", "RF-XP10 0.1.3")
                            + row("Artifact", bytes + " bytes")
                            + "<div class='hash'>SHA-256<br>" + hex(digest.digest()) + "</div>"
                            + "<div class='ok'>Exact .rfplugin packaged successfully</div>");
        } catch (Exception error) {
            return card("Portable plugin", "<div class='bad'>" + escape(error.toString()) + "</div>");
        }
    }

    private String usbCard() {
        UsbManager manager = (UsbManager) getSystemService(Context.USB_SERVICE);
        List<UsbDevice> devices = new ArrayList<>(manager.getDeviceList().values());
        devices.sort(Comparator.comparingInt(UsbDevice::getDeviceId));
        StringBuilder body = new StringBuilder();
        for (UsbDevice device : devices) {
            String name = device.getProductName();
            if (name == null || name.isBlank()) name = device.getDeviceName();
            body.append(device(name,
                    String.format(Locale.ROOT, "VID %04X · PID %04X · %d interface(s)",
                            device.getVendorId(), device.getProductId(), device.getInterfaceCount())));
        }
        if (devices.isEmpty()) body.append(empty("No USB devices detected"));
        return card("USB devices · " + devices.size(), body.toString());
    }

    private String midiCard() {
        MidiManager manager = (MidiManager) getSystemService(Context.MIDI_SERVICE);
        MidiDeviceInfo[] devices = manager == null ? new MidiDeviceInfo[0] : manager.getDevices();
        StringBuilder body = new StringBuilder();
        for (MidiDeviceInfo device : devices) {
            Bundle properties = device.getProperties();
            String name = properties.getString(MidiDeviceInfo.PROPERTY_PRODUCT);
            if (name == null) name = properties.getString(MidiDeviceInfo.PROPERTY_NAME, "MIDI device");
            body.append(device(name, device.getPorts().length + " port(s) · type " + device.getType()));
        }
        if (devices.length == 0) body.append(empty("No MIDI devices detected"));
        return card("MIDI devices · " + devices.length, body.toString());
    }

    private String audioCard() {
        AudioManager manager = (AudioManager) getSystemService(Context.AUDIO_SERVICE);
        AudioDeviceInfo[] outputs = manager.getDevices(AudioManager.GET_DEVICES_OUTPUTS);
        StringBuilder body = new StringBuilder();
        int usbCount = 0;
        for (AudioDeviceInfo output : outputs) {
            boolean usb = output.getType() == AudioDeviceInfo.TYPE_USB_DEVICE
                    || output.getType() == AudioDeviceInfo.TYPE_USB_HEADSET
                    || output.getType() == AudioDeviceInfo.TYPE_USB_ACCESSORY;
            if (usb) usbCount++;
            String name = output.getProductName().toString();
            String rates = output.getSampleRates().length == 0
                    ? "system-selected rate"
                    : join(output.getSampleRates()) + " Hz";
            body.append(device((usb ? "USB · " : "") + name,
                    rates + " · " + channelSummary(output.getChannelCounts())));
        }
        if (outputs.length == 0) body.append(empty("No audio outputs detected"));
        return card("Audio outputs · " + outputs.length,
                row("USB audio outputs", Integer.toString(usbCount)) + body);
    }

    private static String supported(PackageManager packages, String feature) {
        return packages.hasSystemFeature(feature) ? "Supported" : "Not declared";
    }

    private static String card(String title, String body) {
        return "<section><h2>" + escape(title) + "</h2>" + body + "</section>";
    }

    private static String row(String label, String value) {
        return "<div class='row'><span>" + escape(label) + "</span><strong>" + escape(value) + "</strong></div>";
    }

    private static String device(String name, String detail) {
        return "<div class='device'><strong>" + escape(name) + "</strong><small>" + escape(detail) + "</small></div>";
    }

    private static String empty(String message) {
        return "<div class='empty'>" + escape(message) + "</div>";
    }

    private static String channelSummary(int[] channels) {
        return channels.length == 0 ? "system-selected channels" : join(channels) + " ch";
    }

    private static String join(int[] values) {
        StringBuilder text = new StringBuilder();
        for (int index = 0; index < values.length; index++) {
            if (index > 0) text.append(", ");
            text.append(values[index]);
        }
        return text.toString();
    }

    private static String hex(byte[] bytes) {
        StringBuilder text = new StringBuilder(bytes.length * 2);
        for (byte value : bytes) text.append(String.format(Locale.ROOT, "%02x", value));
        return text.toString();
    }

    private static String escape(String value) {
        return value.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")
                .replace("\"", "&quot;").replace("'", "&#39;");
    }

    private static String css() {
        return "*{box-sizing:border-box}body{margin:0;background:#050f16;color:#e2f2f5;font:15px system-ui,sans-serif}"
                + "main{max-width:760px;margin:auto;padding:28px 18px 52px}.eyebrow{color:#5ce2f5;font-size:12px;font-weight:800;letter-spacing:.18em}"
                + "h1{font-size:34px;margin:8px 0}.lead{color:#8ca6af;line-height:1.55;margin:0 0 22px}"
                + "section{background:#112631;border:1px solid #2a4b57;border-radius:16px;padding:18px;margin:12px 0}"
                + "h2{color:#5ce2f5;font-size:17px;margin:0 0 12px}.row{display:flex;justify-content:space-between;gap:14px;padding:9px 0;border-bottom:1px solid #203e49}"
                + ".row:last-child{border:0}.row span{color:#91a9b1}.device{padding:11px 0;border-bottom:1px solid #203e49}.device:last-child{border:0}"
                + ".device strong,.device small{display:block}.device small{color:#91a9b1;margin-top:5px}.hash{word-break:break-all;color:#91a9b1;font:12px monospace;line-height:1.5;margin-top:12px}"
                + ".ok{color:#64dcb5;margin-top:12px}.bad{color:#f27777}.empty{color:#708a93;padding:8px 0}.foot{color:#708a93;font-size:12px;line-height:1.5;margin-top:18px}";
    }
}
