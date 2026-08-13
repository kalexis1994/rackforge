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
import android.provider.DocumentsContract;
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
import android.widget.ImageView;
import android.widget.LinearLayout;
import android.widget.PopupMenu;
import android.widget.ScrollView;
import android.widget.Spinner;
import android.widget.Switch;
import android.widget.TextView;
import android.widget.Toast;

import java.io.File;
import java.io.FileInputStream;
import java.io.FileOutputStream;
import java.io.InputStream;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.Comparator;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.Set;
import java.util.concurrent.atomic.AtomicInteger;
import java.util.UUID;

import org.json.JSONObject;
import org.json.JSONArray;

public final class MainActivity extends Activity {
    static {
        System.loadLibrary("rackforge_android");
    }

    private static final int SAMPLE_RATE = 48_000;
    private static final int REQUEST_INSTALL_PLUGIN = 4101;
    private static final int REQUEST_SELECT_PLUGIN_RESOURCE = 4102;
    private static final long MAX_PLUGIN_BYTES = 512L * 1024L * 1024L;
    private WebView webView;
    private Spinner audioOutputSpinner;
    private volatile boolean audioRunning;
    private volatile int selectedAudioDeviceId;
    private String selectedAudioDeviceKey = "default";
    private int latencyMode;
    private int outputGainDb;
    private long lastObservedAudioXruns = -1;
    private long lastObservedRenderQueueUnderruns = -1;
    private long lastObservedEngineLockMisses = -1;
    private long lastObservedRenderErrors = -1;
    private long lastObservedMidiDroppedEvents = -1;
    private double lastObservedMaximumCallbackUs = -1;
    private boolean refreshingAudioOutputs;
    private final Handler mainHandler = new Handler(Looper.getMainLooper());
    private final List<AudioOutputChoice> audioOutputChoices = new ArrayList<>();
    private final List<MidiDevice> openMidiDevices = new ArrayList<>();
    private final List<MidiOutputPort> openMidiPorts = new ArrayList<>();
    private final List<MidiInputPort> openMidiDestinations = new ArrayList<>();
    private final Map<MidiInputPort, Integer> openKeyLabDestinations = new LinkedHashMap<>();
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
    private String pluginConfigWebEntry;
    private String pluginWebSurface = "play";
    private String pendingResourceRequestId;
    private String pendingResourceId;
    private String pendingResourceKind;
    private String activePluginName = "No plugin";
    private String activePluginVersion = "";
    private TextView activePluginLabel;
    private LinearLayout playToolbar;
    private TextView playContextLabel;
    private AlertDialog pluginPickerDialog;
    private AlertDialog installedPluginsDialog;
    private android.graphics.Typeface displayTypeface;

    private static native String installPluginFile(String archivePath, String storeRoot);
    private static native String installedPlugins(String storeRoot);
    private static native boolean activateInstalledPlugin(String packageRoot, String storeRoot, String dataRoot);
    private static native String pluginPackageRoot();
    private static native String pluginWebEntry();
    private static native String pluginWebContext();
    private static native boolean selectPluginSound(String soundId);
    private static native String pluginProgramCommand(String method, String paramsJson);
    private static native int loadPluginResource(String resourceId, String filePath);
    private static native String importPluginResourceArchive(
            String importerId, String archivePath, String resourceRoot);
    private static native void sendMidiMessage(int status, int data1, int data2, int length);
    private static native void releaseMidiNotes();
    private static native String keyLabAcquirePlan();
    private static native String keyLabRestorePlan();
    private static native boolean keyLabMatchesUsbDevice(int vendorId, int productId);
    private static native boolean keyLabMatchesProductName(String name);
    private static native boolean keyLabMatchesEndpointName(String name);
    private static native String keyLabHandleMidi(int status, int data1, int data2);
    private static native String keyLabPollLongPress();
    private static native boolean keyLabSyncPlugins(String storeRoot);
    private static native boolean keyLabSyncActivePlugin();
    private static native boolean keyLabSyncActiveMode(String mode);
    private static native String keyLabRenderPlan();
    private static native boolean startNativeAudio(int deviceId, int latencyMode);
    private static native void setNativeOutputGain(int gainDb);
    static native void stopNativeAudio();
    private static native String nativeAudioStatus();
    private static native boolean growNativeAudioBuffer();
    private static native int pollNativeAudioError();

    private final Runnable audioHealthPoll = new Runnable() {
        @Override public void run() {
            if (audioRunning) {
                int error = pollNativeAudioError();
                if (error != 0) {
                    recoverAudioStream(error);
                } else {
                    stabilizeAudioBufferAfterXrun();
                }
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
        currentPage = "live".equals(preferences.getString("session.active_mode", "play"))
                ? "live" : "play";
        selectedAudioDeviceKey = preferences.getString("audio.output", "default");
        // Balanced keeps a render-ahead queue for measured portable WASM CPU
        // spikes. Users can still opt into the more aggressive Low profile.
        latencyMode = preferences.getInt("audio.latency", 1);
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
        if ("live".equals(currentPage)) showLive();
        else showPlay();
        startEngine();
    }

    @Override
    protected void onResume() {
        super.onResume();
        refreshAudioOutputs();
        scheduleMidiReconnect();
        restoreVisiblePage();
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
                + "window.postMessage(" + currentPluginWebContext() + ",window.location.origin);"
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
                if ("plugin.select_resource".equals(method)) {
                    String resourceId = request.getJSONObject("params").getString("resource_id");
                    if (!"config".equals(pluginWebSurface) || pluginResourceKind(resourceId) == null) {
                        respondToPlugin(requestId, false, "Resource is not available to this surface.");
                        return;
                    }
                    runOnUiThread(() -> choosePluginResource(requestId, resourceId));
                    return;
                }
                if ("plugin.resource_bindings".equals(method)) {
                    if (!"config".equals(pluginWebSurface)) {
                        respondToPlugin(requestId, false, "Resource bindings require CONFIG.");
                        return;
                    }
                    respondToPlugin(requestId, true, null, pluginResourceBindings());
                    return;
                }
                if ("plugin.resource_status".equals(method)) {
                    if (!"config".equals(pluginWebSurface)) {
                        respondToPlugin(requestId, false, "Resource status requires CONFIG.");
                        return;
                    }
                    respondToPlugin(requestId, true, null, pluginResourceStatus());
                    return;
                }
                if ("plugin.resource_entries".equals(method)) {
                    if (!"config".equals(pluginWebSurface)) {
                        respondToPlugin(requestId, false, "Resource browsing requires CONFIG.");
                        return;
                    }
                    JSONObject params = request.getJSONObject("params");
                    String grantId = params.getString("grant_id");
                    String parentId = params.isNull("parent_id") ? null : params.optString("parent_id", null);
                    new Thread(() -> {
                        try {
                            JSONArray entries = pluginResourceEntries(grantId, parentId);
                            runOnUiThread(() -> respondToPlugin(requestId, true, null, entries));
                        } catch (Throwable error) {
                            Log.e("RackForge", "Could not browse plugin resource", error);
                            runOnUiThread(() -> respondToPlugin(requestId, false,
                                    error.getMessage() == null ? error.toString() : error.getMessage()));
                        }
                    }, "rackforge-resource-browser").start();
                    return;
                }
                if ("plugin.load_resource".equals(method)
                        || "plugin.install_resource".equals(method)) {
                    if (!"config".equals(pluginWebSurface)) {
                        respondToPlugin(requestId, false, "Resource loading requires CONFIG.");
                        return;
                    }
                    JSONObject params = request.getJSONObject("params");
                    String targetResourceId = params.getString("target_resource_id");
                    String grantId = params.getString("grant_id");
                    String entryId = params.isNull("entry_id")
                            ? null : params.optString("entry_id", null);
                    if (!"file".equals(pluginResourceKind(targetResourceId))) {
                        respondToPlugin(requestId, false, "Target is not a declared file resource.");
                        return;
                    }
                    new Thread(() -> {
                        File destination = null;
                        File backup = null;
                        try {
                            String pluginId = new JSONObject(pluginWebContext())
                                    .getJSONObject("instance").getString("plugin_id");
                            JSONArray importTargets = pluginResourceImportTargets(targetResourceId);
                            if (importTargets.length() > 0) {
                                File archive = copyGrantedResourceToPrivateData(
                                        grantId, entryId, targetResourceId);
                                JSONObject result;
                                try {
                                    String imported = importPluginResourceArchive(
                                            targetResourceId,
                                            archive.getAbsolutePath(),
                                            new File(pluginDataRoot(), "resources").getAbsolutePath());
                                    if (imported == null || imported.isBlank()) {
                                        throw new IllegalStateException(
                                                "Portable runtime could not import the resource archive");
                                    }
                                    result = new JSONObject(imported);
                                } finally {
                                    if (archive.isFile() && !archive.delete()) {
                                        Log.w("RackForge", "Could not remove temporary resource archive " + archive);
                                    }
                                }
                                JSONArray installedIds = result.optJSONArray("installed_resource_ids");
                                if (installedIds == null || installedIds.length() == 0) {
                                    throw new IllegalStateException(
                                            "The archive did not contain any recognized plugin resources");
                                }
                                rememberImportedResources(pluginId, grantId, installedIds);
                                runOnUiThread(() -> respondToPlugin(requestId, true, null, result));
                                return;
                            }
                            destination = privatePluginResourceFile(pluginId, targetResourceId);
                            if (destination.isFile()) {
                                backup = new File(destination.getParentFile(),
                                        destination.getName() + ".backup-" + UUID.randomUUID());
                                if (!destination.renameTo(backup)) {
                                    throw new IllegalStateException(
                                            "Could not preserve the previous plugin resource");
                                }
                            }
                            File resource = copyGrantedResourceToPrivateData(
                                    grantId, entryId, targetResourceId);
                            int loadStatus = loadPluginResource(
                                    targetResourceId, resource.getAbsolutePath());
                            if (loadStatus == 0) {
                                throw new IllegalStateException("Portable runtime rejected the resource");
                            }
                            if (backup != null && backup.isFile() && !backup.delete()) {
                                Log.w("RackForge", "Could not remove old plugin resource " + backup);
                            }
                            preferences.edit()
                                    .putString("resource.active_grant." + pluginId + "." + targetResourceId,
                                            grantId)
                                    .putString("resource.active_entry." + pluginId + "." + targetResourceId,
                                            entryId == null ? "__grant_root__" : entryId)
                                    .apply();
                            JSONObject result = new JSONObject();
                            result.put("stored", true);
                            result.put("activated", loadStatus == 1);
                            runOnUiThread(() -> respondToPlugin(requestId, true, null, result));
                        } catch (Throwable error) {
                            Log.e("RackForge", "Could not load plugin resource", error);
                            if (backup != null) {
                                if (backup.isFile()) {
                                    if (destination != null && destination.isFile()
                                            && !destination.delete()) {
                                        Log.e("RackForge", "Could not discard rejected plugin resource "
                                                + destination);
                                    } else if (destination != null && !backup.renameTo(destination)) {
                                        Log.e("RackForge", "Could not restore previous plugin resource "
                                                + backup);
                                    }
                                }
                            } else if (destination != null && destination.isFile()
                                    && !destination.delete()) {
                                Log.e("RackForge", "Could not discard rejected plugin resource "
                                        + destination);
                            }
                            runOnUiThread(() -> respondToPlugin(requestId, false,
                                    error.getMessage() == null ? error.toString() : error.getMessage()));
                        }
                    }, "rackforge-resource-loader").start();
                    return;
                }
                if (isPluginProgramMethod(method)) {
                    JSONObject params = request.optJSONObject("params");
                    String paramsJson = params == null ? "{}" : params.toString();
                    new Thread(() -> {
                        try {
                            String updatedContext = pluginProgramCommand(method, paramsJson);
                            if (updatedContext == null || updatedContext.isBlank()) {
                                throw new IllegalStateException(
                                        "Portable runtime did not return the updated program context");
                            }
                            runOnUiThread(() -> {
                                respondToPlugin(requestId, true, null);
                                sendPluginMessage(updatedContext);
                                if ("plugin.save_program".equals(method)
                                        || "plugin.cancel_program".equals(method)) {
                                    rememberActivePluginSound();
                                    keyLabSyncActivePlugin();
                                    refreshKeyLabDisplay();
                                }
                            });
                        } catch (Throwable error) {
                            Log.e("RackForge", "Plugin program command failed", error);
                            runOnUiThread(() -> respondToPlugin(requestId, false,
                                    error.getMessage() == null ? error.toString() : error.getMessage()));
                        }
                    }, "rackforge-program-editor").start();
                    return;
                }
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
                        rememberActivePluginSound();
                        if (!keyLabSyncActivePlugin()) {
                            throw new IllegalStateException("The controller did not accept the selected sound");
                        }
                        refreshKeyLabDisplay();
                        Log.i("RackForge", "Selected plugin sound " + soundId);
                        runOnUiThread(() -> {
                            respondToPlugin(requestId, true, null);
                            sendPluginMessage(currentPluginWebContext());
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

    private static boolean isPluginProgramMethod(String method) {
        return "plugin.begin_program_edit".equals(method)
                || "plugin.edit_program_field".equals(method)
                || "plugin.set_program_name".equals(method)
                || "plugin.restore_program_preview".equals(method)
                || "plugin.save_program".equals(method)
                || "plugin.cancel_program".equals(method);
    }

    private void respondToPlugin(String requestId, boolean ok, String error) {
        respondToPlugin(requestId, ok, error, null);
    }

    private void respondToPlugin(String requestId, boolean ok, String error, Object result) {
        if (Looper.myLooper() != Looper.getMainLooper()) {
            mainHandler.post(() -> respondToPlugin(requestId, ok, error, result));
            return;
        }
        try {
            JSONObject response = new JSONObject();
            response.put("protocol", "rackforge.plugin.web@1");
            response.put("kind", "response");
            response.put("request_id", requestId);
            response.put("ok", ok);
            if (error != null) response.put("error", error);
            if (result != null) response.put("result", result);
            sendPluginMessage(response.toString());
        } catch (Exception exception) {
            Log.e("RackForge", "Could not answer plugin Web message", exception);
        }
    }

    private String currentPluginWebContext() {
        try {
            JSONObject context = new JSONObject(pluginWebContext());
            context.put("surface", pluginWebSurface);
            return context.toString();
        } catch (Exception error) {
            Log.e("RackForge", "Could not build plugin Web context", error);
            return pluginWebContext();
        }
    }

    private String pluginResourceKind(String resourceId) {
        try {
            JSONArray resources = new JSONObject(pluginWebContext()).optJSONArray("resources");
            if (resources == null) return null;
            for (int index = 0; index < resources.length(); index++) {
                JSONObject resource = resources.getJSONObject(index);
                if (resourceId.equals(resource.optString("id"))) {
                    return resource.optString("kind", null);
                }
            }
        } catch (Exception error) {
            Log.e("RackForge", "Could not validate plugin resource", error);
        }
        return null;
    }

    private JSONArray pluginResourceBindings() throws Exception {
        JSONObject context = new JSONObject(pluginWebContext());
        String pluginId = context.getJSONObject("instance").getString("plugin_id");
        JSONArray declared = context.optJSONArray("resources");
        JSONArray bindings = new JSONArray();
        if (declared == null) return bindings;
        for (int index = 0; index < declared.length(); index++) {
            JSONObject resource = declared.getJSONObject(index);
            String resourceId = resource.getString("id");
            String grantId = preferences.getString(
                    "resource.binding." + pluginId + "." + resourceId, null);
            if (grantId == null || preferences.getString("resource.uri." + grantId, null) == null) continue;
            JSONObject grant = new JSONObject();
            grant.put("grant_id", grantId);
            grant.put("resource_id", resourceId);
            grant.put("display_name", preferences.getString(
                    "resource.name." + grantId, "Authorized storage"));
            grant.put("kind", preferences.getString(
                    "resource.kind." + grantId, resource.optString("kind", "directory")));
            bindings.put(grant);
        }
        return bindings;
    }

    private JSONArray pluginResourceStatus() throws Exception {
        JSONObject context = new JSONObject(pluginWebContext());
        String pluginId = context.getJSONObject("instance").getString("plugin_id");
        JSONArray declared = context.optJSONArray("resources");
        JSONArray statuses = new JSONArray();
        if (declared == null) return statuses;
        for (int index = 0; index < declared.length(); index++) {
            JSONObject resource = declared.getJSONObject(index);
            if (!"file".equals(resource.optString("kind"))) continue;
            String resourceId = resource.getString("id");
            boolean installed = preferences.getString(
                    "resource.active_entry." + pluginId + "." + resourceId, null) != null
                    && privatePluginResourceFile(pluginId, resourceId).isFile();
            JSONObject status = new JSONObject();
            status.put("resource_id", resourceId);
            status.put("installed", installed);
            statuses.put(status);
        }
        return statuses;
    }

    private JSONArray pluginResourceImportTargets(String resourceId) throws Exception {
        JSONObject context = new JSONObject(pluginWebContext());
        JSONArray declared = context.optJSONArray("resources");
        if (declared == null) return new JSONArray();
        for (int index = 0; index < declared.length(); index++) {
            JSONObject resource = declared.getJSONObject(index);
            if (resourceId.equals(resource.optString("id"))) {
                JSONArray targets = resource.optJSONArray("import_targets");
                return targets == null ? new JSONArray() : targets;
            }
        }
        return new JSONArray();
    }

    private void rememberImportedResources(
            String pluginId, String grantId, JSONArray installedIds) throws Exception {
        SharedPreferences.Editor editor = preferences.edit();
        for (int index = 0; index < installedIds.length(); index++) {
            String installedId = installedIds.getString(index);
            editor.putString(
                    "resource.active_grant." + pluginId + "." + installedId,
                    grantId);
            editor.putString(
                    "resource.active_entry." + pluginId + "." + installedId,
                    "__archive_import__");
        }
        editor.apply();
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
        ImageView mark = new ImageView(this);
        mark.setImageResource(R.drawable.rackforge_mark);
        mark.setContentDescription(null);
        mark.setImportantForAccessibility(View.IMPORTANT_FOR_ACCESSIBILITY_NO);
        mark.setPadding(dp(2), dp(2), dp(2), dp(2));
        bar.addView(mark, new LinearLayout.LayoutParams(dp(32), dp(32)));
        TextView title = new TextView(this);
        // The zero-width space is the only intentional wrap opportunity in the
        // wordmark, keeping a narrow header readable as RACK / FORGE.
        title.setText("RACK\u200BFORGE");
        title.setTextColor(0xFF5CE2F5);
        int screenWidthDp = getResources().getConfiguration().screenWidthDp;
        title.setTextSize(screenWidthDp <= 380 ? 14 : screenWidthDp <= 440 ? 16 : 18);
        title.setMaxLines(2);
        title.setBreakStrategy(android.text.Layout.BREAK_STRATEGY_SIMPLE);
        title.setHyphenationFrequency(android.text.Layout.HYPHENATION_FREQUENCY_NONE);
        applyDisplayTypeface(title);
        title.setPadding(dp(8), 0, dp(12), 0);
        bar.addView(title, new LinearLayout.LayoutParams(
                0, ViewGroup.LayoutParams.WRAP_CONTENT, 1));
        activePluginLabel = new TextView(this);
        activePluginLabel.setText(activePluginDisplayName());
        activePluginLabel.setTextColor(0xFFB8CDD3);
        activePluginLabel.setPadding(dp(8), 0, dp(8), 0);
        activePluginLabel.setSingleLine(true);
        activePluginLabel.setEllipsize(android.text.TextUtils.TruncateAt.END);
        activePluginLabel.setMaxWidth(dp(screenWidthDp <= 380 ? 82 : 116));
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
        applyDisplayTypeface(title);
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
        applyDisplayTypeface(title);
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
        applyDisplayTypeface(title);
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
        if (pluginConfigWebEntry != null) {
            panel.addView(menuAction("◇", "Plugin settings", "Configure libraries and plugin resources", () -> {
                menu.dismiss(); showPluginConfig();
            }));
        }
        panel.addView(menuAction("＋", "Install plugin", "Choose a portable .rfplugin package", () -> {
            menu.dismiss(); choosePluginFile();
        }));
        panel.addView(menuAction("▤", "Installed plugins", "Manage versions and active instrument", () -> {
            menu.dismiss(); showInstalledPluginsDialog();
        }));
        panel.addView(menuAction("i", "About RackForge", "Version and runtime information", () -> {
            menu.dismiss(); showAbout();
        }));

        ScrollView scroll = new ScrollView(this);
        scroll.setBackground(surface(0xFF0D202A, 24, 0xFF2A4B57, 1));
        scroll.setFillViewport(false);
        scroll.setClipToPadding(false);
        scroll.setVerticalScrollBarEnabled(true);
        scroll.setScrollbarFadingEnabled(false);
        scroll.setOverScrollMode(View.OVER_SCROLL_IF_CONTENT_SCROLLS);
        scroll.addView(panel, new ScrollView.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT));
        menu.setContentView(scroll);
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
            if (shown == null) return;
            scroll.post(() -> {
                int maximumHeight = Math.round(
                        getResources().getDisplayMetrics().heightPixels * 0.92f);
                int contentHeight = panel.getMeasuredHeight();
                shown.setLayout(
                        ViewGroup.LayoutParams.MATCH_PARENT,
                        Math.min(contentHeight, maximumHeight));
            });
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
        applyDisplayTypeface(heading);
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

    private void choosePluginResource(String requestId, String resourceId) {
        if (pendingResourceRequestId != null) {
            respondToPlugin(requestId, false, "Another resource selection is already open.");
            return;
        }
        String kind = pluginResourceKind(resourceId);
        if (!"directory".equals(kind) && !"file".equals(kind)) {
            respondToPlugin(requestId, false, "Plugin resource has an unsupported kind.");
            return;
        }
        pendingResourceRequestId = requestId;
        pendingResourceId = resourceId;
        pendingResourceKind = kind;
        Intent intent;
        if ("directory".equals(kind)) {
            intent = new Intent(Intent.ACTION_OPEN_DOCUMENT_TREE);
            intent.addFlags(Intent.FLAG_GRANT_PREFIX_URI_PERMISSION);
        } else {
            intent = new Intent(Intent.ACTION_OPEN_DOCUMENT);
            intent.addCategory(Intent.CATEGORY_OPENABLE);
            intent.setType("*/*");
            intent.putExtra(Intent.EXTRA_MIME_TYPES, new String[] {
                    "application/zip", "application/octet-stream", "application/json",
                    "audio/midi", "audio/x-midi"
            });
        }
        intent.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION
                | Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION);
        startActivityForResult(intent, REQUEST_SELECT_PLUGIN_RESOURCE);
    }

    @Override
    protected void onActivityResult(int requestCode, int resultCode, Intent data) {
        super.onActivityResult(requestCode, resultCode, data);
        if (requestCode == REQUEST_SELECT_PLUGIN_RESOURCE) {
            finishPluginResourceSelection(resultCode, data);
            return;
        }
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

    private void finishPluginResourceSelection(int resultCode, Intent data) {
        String requestId = pendingResourceRequestId;
        String resourceId = pendingResourceId;
        String kind = pendingResourceKind;
        pendingResourceRequestId = null;
        pendingResourceId = null;
        pendingResourceKind = null;
        if (requestId == null) return;
        if (resultCode != RESULT_OK || data == null || data.getData() == null) {
            respondToPlugin(requestId, false, "Resource selection was cancelled by the user.");
            return;
        }
        Uri uri = data.getData();
        try {
            int flags = data.getFlags() & Intent.FLAG_GRANT_READ_URI_PERMISSION;
            if (flags != 0) getContentResolver().takePersistableUriPermission(uri, flags);

            JSONObject context = new JSONObject(pluginWebContext());
            String pluginId = context.getJSONObject("instance").getString("plugin_id");
            String bindingKey = "resource.binding." + pluginId + "." + resourceId;
            String grantId = preferences.getString(bindingKey, null);
            String uriKey = grantId == null ? null : "resource.uri." + grantId;
            if (grantId == null || !uri.toString().equals(preferences.getString(uriKey, null))) {
                grantId = UUID.randomUUID().toString();
                uriKey = "resource.uri." + grantId;
            }
            String displayName = selectedResourceName(uri, kind);
            preferences.edit()
                    .putString(bindingKey, grantId)
                    .putString(uriKey, uri.toString())
                    .putString("resource.kind." + grantId, kind)
                    .putString("resource.name." + grantId, displayName)
                    .apply();

            JSONObject grant = new JSONObject();
            grant.put("grant_id", grantId);
            grant.put("resource_id", resourceId);
            grant.put("display_name", displayName);
            grant.put("kind", kind);
            respondToPlugin(requestId, true, null, grant);
        } catch (Throwable error) {
            Log.e("RackForge", "Could not authorize plugin resource", error);
            respondToPlugin(requestId, false,
                    error.getMessage() == null ? error.toString() : error.getMessage());
        }
    }

    private String selectedResourceName(Uri uri, String kind) {
        Uri document = uri;
        if ("directory".equals(kind)) {
            try {
                document = DocumentsContract.buildDocumentUriUsingTree(
                        uri, DocumentsContract.getTreeDocumentId(uri));
            } catch (Exception ignored) {
                // Some providers expose a useful name directly on the tree URI.
            }
        }
        String name = selectedFileName(document);
        return name == null || name.isBlank() ? "Authorized storage" : name;
    }

    private JSONArray pluginResourceEntries(String grantId, String parentId) throws Exception {
        boolean owned = false;
        JSONArray bindings = pluginResourceBindings();
        for (int index = 0; index < bindings.length(); index++) {
            if (grantId.equals(bindings.getJSONObject(index).optString("grant_id"))) {
                owned = true;
                break;
            }
        }
        if (!owned) throw new IllegalArgumentException("Resource grant belongs to another plugin");
        String treeText = preferences.getString("resource.uri." + grantId, null);
        if (treeText == null || !"directory".equals(
                preferences.getString("resource.kind." + grantId, null))) {
            throw new IllegalArgumentException("Unknown directory grant");
        }
        Uri treeUri = Uri.parse(treeText);
        Uri parentUri;
        if (parentId == null) {
            parentUri = DocumentsContract.buildDocumentUriUsingTree(
                    treeUri, DocumentsContract.getTreeDocumentId(treeUri));
        } else {
            String parentText = preferences.getString(
                    "resource.entry." + grantId + "." + parentId, null);
            if (parentText == null) throw new IllegalArgumentException("Unknown resource entry");
            parentUri = Uri.parse(parentText);
        }
        String parentDocumentId = DocumentsContract.getDocumentId(parentUri);
        Uri children = DocumentsContract.buildChildDocumentsUriUsingTree(
                treeUri, parentDocumentId);
        String[] projection = new String[] {
                DocumentsContract.Document.COLUMN_DOCUMENT_ID,
                DocumentsContract.Document.COLUMN_DISPLAY_NAME,
                DocumentsContract.Document.COLUMN_MIME_TYPE,
                DocumentsContract.Document.COLUMN_SIZE,
                DocumentsContract.Document.COLUMN_LAST_MODIFIED
        };
        JSONArray entries = new JSONArray();
        SharedPreferences.Editor entryHandles = preferences.edit();
        try (Cursor cursor = getContentResolver().query(children, projection, null, null, null)) {
            if (cursor == null) throw new IllegalStateException("Storage provider returned no entries");
            int idColumn = cursor.getColumnIndexOrThrow(DocumentsContract.Document.COLUMN_DOCUMENT_ID);
            int nameColumn = cursor.getColumnIndexOrThrow(DocumentsContract.Document.COLUMN_DISPLAY_NAME);
            int mimeColumn = cursor.getColumnIndexOrThrow(DocumentsContract.Document.COLUMN_MIME_TYPE);
            int sizeColumn = cursor.getColumnIndexOrThrow(DocumentsContract.Document.COLUMN_SIZE);
            int modifiedColumn = cursor.getColumnIndexOrThrow(DocumentsContract.Document.COLUMN_LAST_MODIFIED);
            while (cursor.moveToNext()) {
                String documentId = cursor.getString(idColumn);
                Uri documentUri = DocumentsContract.buildDocumentUriUsingTree(treeUri, documentId);
                String entryId = UUID.nameUUIDFromBytes(
                        documentUri.toString().getBytes(StandardCharsets.UTF_8)).toString();
                boolean directory = DocumentsContract.Document.MIME_TYPE_DIR.equals(
                        cursor.getString(mimeColumn));
                entryHandles.putString(
                        "resource.entry." + grantId + "." + entryId,
                        documentUri.toString());
                JSONObject entry = new JSONObject();
                entry.put("id", entryId);
                entry.put("parent_id", parentId == null ? JSONObject.NULL : parentId);
                entry.put("name", cursor.getString(nameColumn));
                entry.put("kind", directory ? "directory" : "file");
                entry.put("size", cursor.isNull(sizeColumn) ? JSONObject.NULL : cursor.getLong(sizeColumn));
                entry.put("modified_unix_ms", cursor.isNull(modifiedColumn)
                        ? JSONObject.NULL : cursor.getLong(modifiedColumn));
                entry.put("lazy", directory);
                entry.put("can_read", true);
                entries.put(entry);
            }
        }
        entryHandles.apply();
        return entries;
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

    private File copyGrantedResourceToPrivateData(
            String grantId, String entryId, String targetResourceId) throws Exception {
        boolean owned = false;
        JSONArray bindings = pluginResourceBindings();
        for (int index = 0; index < bindings.length(); index++) {
            if (grantId.equals(bindings.getJSONObject(index).optString("grant_id"))) {
                owned = true;
                break;
            }
        }
        if (!owned) throw new IllegalArgumentException("Resource grant belongs to another plugin");
        String uriText;
        if (entryId == null) {
            if (!"file".equals(preferences.getString("resource.kind." + grantId, null))) {
                throw new IllegalArgumentException("A directory grant requires a file entry");
            }
            uriText = preferences.getString("resource.uri." + grantId, null);
        } else {
            uriText = preferences.getString(
                    "resource.entry." + grantId + "." + entryId, null);
        }
        if (uriText == null) throw new IllegalArgumentException("Unknown resource entry");
        JSONObject context = new JSONObject(pluginWebContext());
        String pluginId = context.getJSONObject("instance").getString("plugin_id");
        String safeResource = targetResourceId.replaceAll("[^A-Za-z0-9._-]", "_");
        File destination = privatePluginResourceFile(pluginId, targetResourceId);
        File directory = destination.getParentFile();
        if (!directory.mkdirs() && !directory.isDirectory()) {
            throw new IllegalStateException("Could not create private resource directory");
        }
        File temporary = File.createTempFile(safeResource + "-", ".tmp", directory);
        long total = 0;
        try (InputStream input = getContentResolver().openInputStream(Uri.parse(uriText));
             FileOutputStream output = new FileOutputStream(temporary)) {
            if (input == null) throw new IllegalStateException("The selected resource cannot be opened");
            byte[] buffer = new byte[64 * 1024];
            int read;
            while ((read = input.read(buffer)) >= 0) {
                total += read;
                if (total > MAX_PLUGIN_BYTES) {
                    throw new IllegalStateException("The resource exceeds the 512 MB limit");
                }
                output.write(buffer, 0, read);
            }
            if (total == 0) throw new IllegalStateException("The selected resource is empty");
            output.flush();
            output.getFD().sync();
        } catch (Throwable error) {
            if (temporary.isFile() && !temporary.delete()) temporary.deleteOnExit();
            throw error;
        }
        if (destination.exists() && !destination.delete()) {
            throw new IllegalStateException("Could not replace the previous plugin resource");
        }
        if (!temporary.renameTo(destination)) {
            throw new IllegalStateException("Could not activate the copied plugin resource");
        }
        return destination;
    }

    private File privatePluginResourceFile(String pluginId, String resourceId) {
        String safePlugin = pluginId.replaceAll("[^A-Za-z0-9._-]", "_");
        String safeResource = resourceId.replaceAll("[^A-Za-z0-9._-]", "_");
        return new File(pluginDataRoot(),
                "resources/" + safePlugin + "/" + safeResource + ".resource");
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
            applyDisplayTypeface(title);
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
        if (active && pluginConfigWebEntry != null) {
            Button configure = button("Configure plugin");
            configure.setOnClickListener(view -> {
                if (installedPluginsDialog != null) installedPluginsDialog.dismiss();
                showPluginConfig();
            });
            card.addView(configure, new LinearLayout.LayoutParams(
                    ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT));
        }
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
        pluginConfigWebEntry = instance.optString("config_web_entry", null);
    }

    private void rememberActivePluginSound() {
        try {
            JSONObject instance = new JSONObject(pluginWebContext()).getJSONObject("instance");
            String pluginId = instance.getString("plugin_id");
            String soundId = instance.optString("selected_sound_id", "");
            if (!soundId.isBlank()) {
                preferences.edit()
                        .putString("plugin.selected_sound." + pluginId, soundId)
                        .apply();
            }
        } catch (Throwable error) {
            Log.w("RackForge", "Could not persist the active plugin sound", error);
        }
    }

    private void restorePersistedPluginSound() {
        String preferenceKey = null;
        try {
            JSONObject instance = new JSONObject(pluginWebContext()).getJSONObject("instance");
            String pluginId = instance.getString("plugin_id");
            preferenceKey = "plugin.selected_sound." + pluginId;
            String savedSoundId = preferences.getString(preferenceKey, null);
            if (savedSoundId == null || savedSoundId.isBlank()) {
                rememberActivePluginSound();
                return;
            }
            JSONArray sounds = instance.optJSONArray("sounds");
            boolean available = false;
            if (sounds != null) {
                for (int index = 0; index < sounds.length(); index++) {
                    if (savedSoundId.equals(sounds.getJSONObject(index).optString("id"))) {
                        available = true;
                        break;
                    }
                }
            }
            if (!available || !selectPluginSound(savedSoundId)) {
                preferences.edit().remove(preferenceKey).apply();
                Log.w("RackForge", "Saved plugin sound is unavailable: " + savedSoundId);
                rememberActivePluginSound();
            }
        } catch (Throwable error) {
            if (preferenceKey != null) preferences.edit().remove(preferenceKey).apply();
            Log.w("RackForge", "Could not restore the saved plugin sound", error);
        }
    }

    private void activatePlugin(String root, String name, String version) {
        if (engineStarting) {
            Toast.makeText(this, "RackForge is already changing plugins", Toast.LENGTH_SHORT).show();
            return;
        }
        engineStarting = true;
        currentPage = "play";
        pluginWebSurface = "play";
        showEngineState("Loading " + name,
                "Activating portable plugin " + versionLabel(version) + "…");
        new Thread(() -> {
            File previousRoot = pluginPackageRoot;
            String previousEntry = pluginWebEntry;
            String previousConfigEntry = pluginConfigWebEntry;
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
                restoreActivePluginResources();
                restorePersistedPluginSound();
                if (!keyLabSyncPlugins(pluginStoreRoot().getAbsolutePath())) {
                    throw new IllegalStateException("The controller plugin catalog could not be synchronized");
                }
                startAudio();
                refreshKeyLabDisplay();
                preferences.edit().putString("plugin.active_root", pluginPackageRoot.getAbsolutePath()).apply();
                runOnUiThread(() -> {
                    engineStarting = false;
                    if (activePluginLabel != null) activePluginLabel.setText(activePluginDisplayName());
                    restoreVisiblePage();
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
                        pluginConfigWebEntry = previousConfigEntry;
                        activePluginName = previousName;
                        activePluginVersion = previousVersion;
                        restoreActivePluginResources();
                        restorePersistedPluginSound();
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
            else if ("plugin-config".equals(currentPage)) showPluginConfig();
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
                .setMessage("RackForge Android " + BuildConfig.VERSION_NAME
                        + "\nPortable .rfplugin runtime\nRust + Wasmtime + AAudio")
                .setPositiveButton("Close", null)
                .show();
    }

    /**
     * Refresh native pages after an Activity resume or an asynchronous engine event without
     * reloading the plugin CONFIG WebView. A document picker pauses the Activity; reloading that
     * WebView here would destroy the pending select_resource promise before onActivityResult can
     * deliver the selected grant.
     */
    private void restoreVisiblePage() {
        if ("diagnostics".equals(currentPage)) {
            renderDiagnostics();
        } else if ("live".equals(currentPage)) {
            showLive();
        } else if ("idle".equals(currentPage)) {
            showIdle();
        } else if ("plugin-config".equals(currentPage)) {
            pluginWebSurface = "config";
            updateModeButtons();
        } else {
            showPlay();
        }
    }

    private void syncControllerActiveMode(String mode, boolean persist) {
        try {
            if (!keyLabSyncActiveMode(mode)) {
                Log.w("RackForge", "LITTLE could not synchronize active mode " + mode);
                return;
            }
            if (persist) {
                preferences.edit().putString("session.active_mode", mode).apply();
            }
        } catch (Throwable error) {
            Log.w("RackForge", "LITTLE active mode synchronization failed", error);
        }
    }

    private void showPlay() {
        currentPage = "play";
        pluginWebSurface = "play";
        syncControllerActiveMode("play", true);
        updateModeButtons();
        if (audioRunning && pluginWebEntry != null) {
            webView.loadUrl("https://rackforge.local/plugin/" + pluginWebEntry);
            return;
        }
        if (engineStarting) {
            showEngineState("Loading plugin", "Validating the portable package and starting AAudio…");
        } else {
            showEngineState("No plugin selected",
                    "Install a portable .rfplugin package, then choose it from Select plugin.");
        }
    }

    private void showPluginConfig() {
        if (pluginConfigWebEntry == null) return;
        currentPage = "plugin-config";
        pluginWebSurface = "config";
        syncControllerActiveMode("play", true);
        updateModeButtons();
        webView.loadUrl("https://rackforge.local/plugin/" + pluginConfigWebEntry);
    }

    private void showIdle() {
        currentPage = "idle";
        syncControllerActiveMode("idle", false);
        updateModeButtons();
        showEngineState("RackForge idle",
                "No plugin is active. Choose PLAY and select a plugin to start audio again.");
    }

    private void showLive() {
        currentPage = "live";
        syncControllerActiveMode("live", true);
        updateModeButtons();
        int midiPorts;
        synchronized (openMidiPorts) { midiPorts = openMidiPorts.size(); }
        String body = "<!doctype html><html><head><meta name='viewport' content='width=device-width,initial-scale=1'>"
                + "<style>" + css() + "</style></head><body><main>"
                + "<div class='eyebrow'>LIVE MODE</div><h1>Performance rack</h1>"
                + "<p class='lead'>A stable performance view that stays separate from the plugin editor in Play.</p>"
                + card("Rack 1", row("Instrument", activePluginDisplayName())
                        + row("Audio", audioRunning ? selectedAudioOutputLabel() : "Inactive")
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
        applyDisplayTypeface(title);
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
            long lockMisses = status.optLong("engine_lock_misses", 0);
            long renderErrors = status.optLong("render_errors", 0);
            long queueUnderruns = status.optLong("render_queue_underruns", 0);
            int queuedFrames = status.optInt("render_queue_frames", 0);
            long nonfiniteSamples = status.optLong("nonfinite_samples", 0);
            long clippedSamples = status.optLong("clipped_samples", 0);
            audioCard.addView(settingsValue("Stream", actualRate + " Hz · " + burst + " frames/burst"));
            audioCard.addView(settingsValue("Buffer", buffer + " frames · " + xruns + " xruns"));
            audioCard.addView(settingsValue("AAudio callback", String.format(Locale.ROOT,
                    "%.1f%% · avg %.0f µs · max %.0f µs", load, averageUs, maximumUs)));
            audioCard.addView(settingsValue("Render queue", queuedFrames + " frames · "
                    + queueUnderruns + " underruns"));
            audioCard.addView(settingsValue("Audio continuity", lockMisses + " lock misses · "
                    + renderErrors + " render errors"));
            audioCard.addView(settingsValue("Signal integrity", nonfiniteSamples + " invalid · "
                    + clippedSamples + " clipped samples"));
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
                    restoreVisiblePage();
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
        applyDisplayTypeface(view);
        view.setPadding(0, dp(16), 0, dp(8));
        return view;
    }

    private void applyDisplayTypeface(TextView view) {
        if (displayTypeface == null) {
            displayTypeface = android.graphics.Typeface.createFromAsset(
                    getAssets(), "fonts/ChakraPetch-SemiBold.ttf");
        }
        view.setTypeface(displayTypeface, android.graphics.Typeface.NORMAL);
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
                File store = pluginStoreRoot();
                File data = pluginDataRoot();
                File legacyAddons = new File(data, "addons");
                if (!store.mkdirs() && !store.isDirectory()) throw new IllegalStateException("Cannot create plugin store");
                // Android's scoped external storage may report EACCES instead of ENOENT
                // when the core probes this legacy migration directory.
                if (!legacyAddons.mkdirs() && !legacyAddons.isDirectory()) {
                    throw new IllegalStateException("Cannot create legacy plugin migration directory");
                }
                installBundledDefaultPluginIfEmpty(store);
                List<String> candidates = startupPluginRoots(store);
                Throwable activationError = null;
                boolean activated = false;
                for (String root : candidates) {
                    try {
                        if (!activateInstalledPlugin(root, store.getAbsolutePath(), data.getAbsolutePath())) {
                            throw new IllegalStateException("The installed plugin could not be activated");
                        }
                        activated = true;
                        break;
                    } catch (Throwable candidateError) {
                        activationError = candidateError;
                        Log.w("RackForge", "Installed plugin is unavailable: " + root, candidateError);
                    }
                }
                if (!activated) {
                    preferences.edit().remove("plugin.active_root").apply();
                    if (!keyLabSyncPlugins(store.getAbsolutePath())) {
                        throw new IllegalStateException("The controller plugin catalog could not be synchronized");
                    }
                    String failureDetail = activationError == null
                            ? "Select another installed plugin."
                            : activationError.getMessage() == null
                                    ? activationError.toString()
                                    : activationError.getMessage();
                    runOnUiThread(() -> {
                        engineStarting = false;
                        activePluginName = "No plugin";
                        activePluginVersion = "";
                        pluginPackageRoot = null;
                        pluginWebEntry = null;
                        if (activePluginLabel != null) activePluginLabel.setText(activePluginDisplayName());
                        showEngineState(candidates.isEmpty() ? "No plugins installed" : "No plugin could start",
                                candidates.isEmpty()
                                        ? "Install a portable .rfplugin package to begin."
                                        : failureDetail);
                    });
                    return;
                }
                refreshActivePluginMetadata();
                restoreActivePluginResources();
                restorePersistedPluginSound();
                preferences.edit().putString(
                        "plugin.active_root", pluginPackageRoot.getAbsolutePath()).apply();
                if (!keyLabSyncPlugins(store.getAbsolutePath())) {
                    throw new IllegalStateException("The controller plugin catalog could not be synchronized");
                }
                startAudio();
                openMidiInputs();
                runOnUiThread(() -> {
                    engineStarting = false;
                    if (activePluginLabel != null) activePluginLabel.setText(activePluginDisplayName());
                    restoreVisiblePage();
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

    private void restoreActivePluginResources() throws Exception {
        JSONObject context = new JSONObject(pluginWebContext());
        String pluginId = context.getJSONObject("instance").getString("plugin_id");
        JSONArray resources = context.optJSONArray("resources");
        if (resources == null) return;
        for (int index = 0; index < resources.length(); index++) {
            JSONObject resource = resources.getJSONObject(index);
            if (!"file".equals(resource.optString("kind"))) continue;
            String resourceId = resource.getString("id");
            String activeEntry = preferences.getString(
                    "resource.active_entry." + pluginId + "." + resourceId, null);
            if (activeEntry == null) continue;
            File copy = privatePluginResourceFile(pluginId, resourceId);
            if (!copy.isFile()) continue;
            JSONArray importTargets = resource.optJSONArray("import_targets");
            if (importTargets != null && importTargets.length() > 0) {
                String imported = importPluginResourceArchive(
                        resourceId,
                        copy.getAbsolutePath(),
                        new File(pluginDataRoot(), "resources").getAbsolutePath());
                if (imported == null || imported.isBlank()) {
                    throw new IllegalStateException(
                            "Could not migrate installed resource archive " + resourceId);
                }
                JSONArray installedIds = new JSONObject(imported)
                        .getJSONArray("installed_resource_ids");
                rememberImportedResources(
                        pluginId,
                        preferences.getString(
                                "resource.active_grant." + pluginId + "." + resourceId,
                                "__legacy_archive__"),
                        installedIds);
                preferences.edit()
                        .remove("resource.active_grant." + pluginId + "." + resourceId)
                        .remove("resource.active_entry." + pluginId + "." + resourceId)
                        .apply();
                if (!copy.delete()) {
                    Log.w("RackForge", "Could not remove migrated resource archive " + copy);
                }
                continue;
            }
            if (loadPluginResource(resourceId, copy.getAbsolutePath()) == 0) {
                throw new IllegalStateException("Could not restore plugin resource " + resourceId);
            }
        }
    }

    private List<String> startupPluginRoots(File store) throws Exception {
        List<String> roots = new ArrayList<>();
        String preferred = preferences.getString("plugin.active_root", null);
        if (preferred != null && !preferred.isBlank()) roots.add(preferred);
        JSONObject catalog = new JSONObject(installedPlugins(store.getAbsolutePath()));
        JSONArray plugins = catalog.getJSONArray("plugins");
        for (int index = 0; index < plugins.length(); index++) {
            JSONObject plugin = plugins.getJSONObject(index);
            if (!plugin.optBoolean("compatible")) continue;
            String root = plugin.optString("package_root", "");
            if (!root.isBlank() && !roots.contains(root)) roots.add(root);
        }
        return roots;
    }

    private void installBundledDefaultPluginIfEmpty(File store) throws Exception {
        JSONObject catalog = new JSONObject(installedPlugins(store.getAbsolutePath()));
        if (catalog.getJSONArray("plugins").length() != 0) return;

        String assetName = "RF-Soundfonts.rfplugin";
        boolean available = false;
        String[] bundled = getAssets().list("bundled");
        if (bundled != null) {
            for (String name : bundled) {
                if (assetName.equals(name)) {
                    available = true;
                    break;
                }
            }
        }
        if (!available) return;

        File archive = new File(getCacheDir(), "rackforge-default-plugin.rfplugin");
        try (InputStream input = getAssets().open("bundled/" + assetName);
             FileOutputStream output = new FileOutputStream(archive)) {
            byte[] buffer = new byte[64 * 1024];
            int read;
            while ((read = input.read(buffer)) != -1) output.write(buffer, 0, read);
            output.getFD().sync();
        }
        try {
            String descriptor = installPluginFile(
                    archive.getAbsolutePath(), store.getAbsolutePath());
            if (descriptor == null || descriptor.isBlank()) {
                throw new IllegalStateException("The bundled default instrument was rejected");
            }
            JSONObject installed = new JSONObject(descriptor);
            Log.i("RackForge", "Bundled default plugin ready: "
                    + installed.optString("plugin_name", "RF-Soundfonts"));
        } finally {
            if (!archive.delete() && archive.exists()) archive.deleteOnExit();
        }
    }

    private void startAudio() {
        lastObservedAudioXruns = -1;
        lastObservedRenderQueueUnderruns = -1;
        lastObservedEngineLockMisses = -1;
        lastObservedRenderErrors = -1;
        lastObservedMidiDroppedEvents = -1;
        lastObservedMaximumCallbackUs = -1;
        if (!startNativeAudio(selectedAudioDeviceId, latencyMode)) {
            throw new IllegalStateException("Native low-latency audio rejected the selected output");
        }
        audioRunning = true;
        startAudioService();
    }

    private void stabilizeAudioBufferAfterXrun() {
        try {
            JSONObject status = new JSONObject(nativeAudioStatus());
            long xruns = status.optLong("xruns", -1);
            long renderQueueUnderruns = status.optLong("render_queue_underruns", -1);
            long engineLockMisses = status.optLong("engine_lock_misses", -1);
            long renderErrors = status.optLong("render_errors", -1);
            long midiDroppedEvents = status.optLong("midi_dropped_events", -1);
            double maximumCallbackUs = status.optDouble("maximum_callback_us", -1);
            double callbackBudgetUs = status.optDouble("callback_budget_us", -1);
            if (lastObservedAudioXruns < 0) {
                Log.i("RackForge", "Audio continuity baseline: buffer="
                        + status.optInt("buffer_size_frames", -1) + "/"
                        + status.optInt("buffer_capacity_frames", -1)
                        + " frames burst=" + status.optInt("frames_per_burst", -1)
                        + " callbackFrames="
                        + status.optInt("frames_per_data_callback", -1)
                        + " callbackBudgetUs=" + Math.round(callbackBudgetUs)
                        + " callbackLoadPercent="
                        + Math.round(status.optDouble("callback_load_percent", -1))
                        + " renderQueueFrames=" + status.optInt("render_queue_frames", -1)
                        + " renderThreadPriorityResult="
                        + status.optInt("render_thread_priority_result", -1)
                        + " sharingMode=" + status.optInt("sharing_mode", -1)
                        + " performanceMode=" + status.optInt("performance_mode", -1));
            }
            if (renderQueueUnderruns >= 0 && lastObservedRenderQueueUnderruns >= 0
                    && renderQueueUnderruns > lastObservedRenderQueueUnderruns) {
                Log.w("RackForge", "Audio continuity render queue underrun: delta="
                        + (renderQueueUnderruns - lastObservedRenderQueueUnderruns)
                        + " total=" + renderQueueUnderruns
                        + " missingFrames="
                        + status.optLong("render_queue_underrun_frames", -1));
            }
            if (xruns >= 0 && lastObservedAudioXruns >= 0 && xruns > lastObservedAudioXruns) {
                int grownBursts = 0;
                while (grownBursts < 3 && growNativeAudioBuffer()) grownBursts++;
                if (grownBursts > 0) {
                    Log.w("RackForge", "Audio continuity xrun: delta="
                            + (xruns - lastObservedAudioXruns) + " total=" + xruns
                            + "; increased buffer by " + grownBursts + " burst(s)");
                } else {
                    Log.w("RackForge", "Audio continuity xrun: delta="
                            + (xruns - lastObservedAudioXruns) + " total=" + xruns
                            + "; buffer cannot grow further");
                }
            }
            if (engineLockMisses >= 0 && lastObservedEngineLockMisses >= 0
                    && engineLockMisses > lastObservedEngineLockMisses) {
                Log.w("RackForge", "Audio continuity engine lock miss: delta="
                        + (engineLockMisses - lastObservedEngineLockMisses)
                        + " total=" + engineLockMisses);
            }
            if (renderErrors >= 0 && lastObservedRenderErrors >= 0
                    && renderErrors > lastObservedRenderErrors) {
                Log.w("RackForge", "Audio continuity render error: delta="
                        + (renderErrors - lastObservedRenderErrors)
                        + " total=" + renderErrors);
            }
            if (midiDroppedEvents >= 0 && lastObservedMidiDroppedEvents >= 0
                    && midiDroppedEvents > lastObservedMidiDroppedEvents) {
                Log.w("RackForge", "Audio continuity MIDI queue overflow: delta="
                        + (midiDroppedEvents - lastObservedMidiDroppedEvents)
                        + " total=" + midiDroppedEvents);
            }
            if (maximumCallbackUs > callbackBudgetUs && callbackBudgetUs > 0
                    && maximumCallbackUs > lastObservedMaximumCallbackUs) {
                Log.w("RackForge", "Audio continuity slow callback: maximumUs="
                        + Math.round(maximumCallbackUs) + " budgetUs="
                        + Math.round(callbackBudgetUs));
            }
            lastObservedAudioXruns = xruns;
            lastObservedRenderQueueUnderruns = renderQueueUnderruns;
            lastObservedEngineLockMisses = engineLockMisses;
            lastObservedRenderErrors = renderErrors;
            lastObservedMidiDroppedEvents = midiDroppedEvents;
            lastObservedMaximumCallbackUs = Math.max(
                    lastObservedMaximumCallbackUs, maximumCallbackUs);
        } catch (Throwable error) {
            Log.w("RackForge", "Could not inspect AAudio continuity", error);
        }
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
        private final int generation;
        private int runningStatus = -1;
        private int messageStatus = -1;
        private int expectedDataBytes;
        private int dataCount;
        private final byte[] messageData = new byte[2];
        private boolean inSysEx;

        MidiStreamDecoder(boolean keyLabSurface, boolean forwardMidi, int generation) {
            this.keyLabSurface = keyLabSurface;
            this.forwardMidi = forwardMidi;
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
                    handleKeyLabResponse(response, generation);
                    if (data1 >= 44 && data1 <= 47 && data2 == 127) {
                        scheduleKeyLabLongPress(generation);
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

    private void handleKeyLabResponse(String json, int generation) {
        try {
            JSONObject response = new JSONObject(json);
            sendControllerPlanToKeyLab(response.getJSONArray("plan").toString(), generation);
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

    private void scheduleKeyLabLongPress(int generation) {
        mainHandler.postDelayed(() -> {
            if (generation != midiGeneration) return;
            String response = keyLabPollLongPress();
            if (response != null) handleKeyLabResponse(response, generation);
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
            rememberActivePluginSound();
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
            String deviceName = midiDeviceName(info);
            boolean performanceEnabled = enabledInputs == null || enabledInputs.contains(deviceName);
            boolean keyLab = isKeyLabDevice(info, manager);
            // A RackForge controller is a control-plane device even when the user
            // has disabled it as a musical input. LITTLE must still be acquired.
            if (!performanceEnabled && !keyLab) continue;
            manager.openDevice(info, device -> {
                if (device == null) return;
                if (!audioRunning || generation != midiGeneration) {
                    try { device.close(); } catch (Exception ignored) { }
                    return;
                }
                synchronized (openMidiDevices) { openMidiDevices.add(device); }
                if (keyLab) openKeyLabDestinations(device, info, generation);
                for (MidiDeviceInfo.PortInfo portInfo : info.getPorts()) {
                    if (portInfo.getType() != MidiDeviceInfo.PortInfo.TYPE_OUTPUT) continue;
                    boolean keyLabPrimary = keyLab && isPrimaryKeyLabPort(portInfo);
                    boolean forwardMidi = performanceEnabled && (!keyLab || keyLabPrimary);
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
                                keyLabPrimary, forwardMidi, generation);

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

    private void openKeyLabDestinations(MidiDevice device, MidiDeviceInfo info,
            int generation) {
        List<MidiDeviceInfo.PortInfo> inputs = new ArrayList<>();
        List<MidiDeviceInfo.PortInfo> namedMatches = new ArrayList<>();
        for (MidiDeviceInfo.PortInfo portInfo : info.getPorts()) {
            if (portInfo.getType() != MidiDeviceInfo.PortInfo.TYPE_INPUT) continue;
            inputs.add(portInfo);
            String name = portInfo.getName();
            if (name != null && !name.isBlank() && keyLabMatchesEndpointName(name)) {
                namedMatches.add(portInfo);
            }
        }
        if (inputs.isEmpty()) {
            Log.w("RackForge", "KeyLab detected without a MIDI destination port");
            return;
        }

        // Android often strips the USB MIDI jack names and publishes four anonymous
        // virtual cables for the KeyLab. In that case there is no API-level way to
        // distinguish MIDI from DINTHRU/MCU/HUI/ALV, so address every cable. Only the
        // controller's private control cable consumes RackForge SysEx messages.
        List<MidiDeviceInfo.PortInfo> targets = namedMatches.isEmpty() ? inputs : namedMatches;
        String acquirePlan = keyLabAcquirePlan();
        int opened = 0;
        for (MidiDeviceInfo.PortInfo target : targets) {
            MidiInputPort port = device.openInputPort(target.getPortNumber());
            if (port == null) {
                Log.w("RackForge", "Could not open KeyLab destination port "
                        + target.getPortNumber());
                continue;
            }
            if (!audioRunning || generation != midiGeneration) {
                try { port.close(); } catch (Exception ignored) { }
                continue;
            }
            synchronized (openMidiDestinations) { openMidiDestinations.add(port); }
            synchronized (openKeyLabDestinations) {
                openKeyLabDestinations.put(port, target.getPortNumber());
            }
            long initialDelayMs = opened * 750L;
            sendControllerPlan(port, target.getPortNumber(), acquirePlan, generation,
                    initialDelayMs);
            Log.i("RackForge", "KeyLab LITTLE destination port "
                    + target.getPortNumber() + " name=" + target.getName()
                    + " acquireDelayMs=" + initialDelayMs);
            opened++;
        }
        if (opened == 0) {
            Log.w("RackForge", "Could not open any KeyLab MIDI destination");
            return;
        }
        Log.i("RackForge", "KeyLab controller runtime acquired on Android using "
                + opened + " destination port(s)");
    }

    private void sendControllerPlan(MidiInputPort port, int destinationPort, String json,
            int generation, long initialDelayMs) {
        try {
            JSONArray plan = new JSONArray(json);
            long delayMs = initialDelayMs;
            for (int index = 0; index < plan.length(); index++) {
                JSONObject step = plan.getJSONObject(index);
                JSONArray values = step.getJSONArray("bytes");
                byte[] message = new byte[values.length()];
                for (int byteIndex = 0; byteIndex < values.length(); byteIndex++) {
                    message[byteIndex] = (byte) values.getInt(byteIndex);
                }
                long scheduledAt = delayMs;
                boolean lastStep = index == plan.length() - 1;
                mainHandler.postDelayed(() -> {
                    if (generation != midiGeneration) return;
                    synchronized (openMidiDestinations) {
                        if (!openMidiDestinations.contains(port)) return;
                    }
                    try {
                        port.send(message, 0, message.length);
                        if (lastStep) {
                            Log.i("RackForge", "KeyLab plan completed on destination port "
                                    + destinationPort);
                        }
                    } catch (Exception error) {
                        Log.e("RackForge", "KeyLab MIDI output failed on destination port "
                                + destinationPort, error);
                    }
                }, scheduledAt);
                delayMs += step.optLong("settle_after_ms", 0);
            }
            Log.i("RackForge", "KeyLab plan scheduled on destination port "
                    + destinationPort + " steps=" + plan.length()
                    + " initialDelayMs=" + initialDelayMs);
        } catch (Exception error) {
            Log.e("RackForge", "Invalid KeyLab controller plan", error);
        }
    }

    private void sendControllerPlanToKeyLab(String plan, int generation) {
        synchronized (openKeyLabDestinations) {
            for (Map.Entry<MidiInputPort, Integer> destination
                    : openKeyLabDestinations.entrySet()) {
                sendControllerPlan(destination.getKey(), destination.getValue(), plan,
                        generation, 0);
            }
        }
    }

    private void refreshKeyLabDisplay() {
        String plan = keyLabRenderPlan();
        if (plan == null) return;
        sendControllerPlanToKeyLab(plan, midiGeneration);
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

    private boolean isKeyLabDevice(MidiDeviceInfo info, MidiManager midiManager) {
        Bundle properties = info.getProperties();
        String product = properties.getString(MidiDeviceInfo.PROPERTY_PRODUCT, "");
        String name = properties.getString(MidiDeviceInfo.PROPERTY_NAME, "");
        if (keyLabMatchesProductName(product) || keyLabMatchesProductName(name)) return true;

        boolean physicalMatch = hasSupportedKeyLabUsbDevice();
        if (!physicalMatch) return false;
        if (keyLabMatchesEndpointName(product) || keyLabMatchesEndpointName(name)) return true;

        String manufacturer = properties.getString(MidiDeviceInfo.PROPERTY_MANUFACTURER, "");
        if (manufacturer.toLowerCase(Locale.ROOT).contains("arturia")) return true;

        // Some Android MIDI drivers publish no useful product/manufacturer text.
        // A single USB MIDI service alongside the declared VID/PID is unambiguous.
        int usbMidiDevices = 0;
        for (MidiDeviceInfo candidate : midiManager.getDevices()) {
            if (candidate.getType() == MidiDeviceInfo.TYPE_USB) usbMidiDevices++;
        }
        return usbMidiDevices == 1;
    }

    private boolean hasSupportedKeyLabUsbDevice() {
        UsbManager manager = (UsbManager) getSystemService(Context.USB_SERVICE);
        if (manager == null) return false;
        for (UsbDevice device : manager.getDeviceList().values()) {
            if (keyLabMatchesUsbDevice(device.getVendorId(), device.getProductId())) return true;
        }
        return false;
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
            for (MidiInputPort port : openKeyLabDestinations.keySet()) {
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
                + "<p class='foot'>Install and select a portable plugin, then play the connected USB MIDI controller.</p>"
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
        try {
            JSONObject catalog = new JSONObject(installedPlugins(pluginStoreRoot().getAbsolutePath()));
            JSONArray plugins = catalog.getJSONArray("plugins");
            return card("Portable plugin",
                    row("Installed", Integer.toString(plugins.length()))
                            + row("Active", audioRunning ? activePluginDisplayName() : "None")
                            + "<div class='ok'>Plugins are installed independently from RackForge.</div>");
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
