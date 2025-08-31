import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/tauri";
import { AutoDetectModal } from "./components/AutoDetectModal";
import "./styles.css";

interface TemperatureSensor {
  name: string;
  input_path: string;
  label?: string | null;
  current_temp?: number | null;
}

interface FanSensor {
  name: string;
  input_path: string;
  label?: string | null;
  current_rpm?: number | null;
}

interface PwmController {
  name: string;
  pwm_path: string;
  enable_path: string;
  label?: string | null;
  current_value?: number | null;
  current_percent?: number | null;
}

interface HwmonChip {
  name: string;
  path: string;
  temperatures: TemperatureSensor[];
  fans: FanSensor[];
  pwms: PwmController[];
}

interface TempSource {
  sensor_path: string;
  sensor_name: string;
  sensor_label?: string;
  current_temp?: number;
  chip_name: string;
}

interface FanMapping {
  fan_name: string;
  pwm_name: string;
  confidence: number;
  temp_sources: TempSource[];
  response_time_ms?: number;
  min_pwm?: number;
  max_rpm?: number;
  selected_temp_source?: string;
}


function App() {
  const [activeTab, setActiveTab] = useState("fancontrol");
  const [chips, setChips] = useState<HwmonChip[] | null>(null);
  const [sensorNames, setSensorNames] = useState<Record<string, string>>({});
  const [mappings, setMappings] = useState<FanMapping[]>([]);
  const [showAutoDetectModal, setShowAutoDetectModal] = useState(false);

  const fetchHwmon = async () => {
    try {
      const data = await invoke<HwmonChip[]>("get_hwmon_chips");
      setChips(data);
    } catch (e) {
      console.error(e);
    }
  };

  const fetchSensorNames = async () => {
    try {
      const data = await invoke<Record<string, string>>("get_sensor_names_cmd");
      setSensorNames(data || {});
    } catch (e) {
      console.error(e);
    }
  };

  const handleAutoDetectConfirm = async (detectedMappings: FanMapping[]) => {
    setMappings(detectedMappings);
    await fetchHwmon();
    // Optionally save the mappings to profile here
  };


  // Initial load sensors and names
  useEffect(() => {
    fetchHwmon();
    fetchSensorNames();
  }, []);

  const tabs = [
    { id: "fancontrol", label: "Fan Control", enabled: true },
    { id: "sensors", label: "Sensors", enabled: true },
    { id: "profiles", label: "Profile Manager", enabled: true },
  ];

  return (
    <div className="min-h-screen bg-gray-900 text-gray-100">
      {/* Header */}
      <header className="bg-gray-800 border-b border-gray-700">
        <div className="container mx-auto px-4 py-4">
          <h1 className="text-2xl font-bold text-blue-400">Hyperfan Control</h1>
        </div>
      </header>

      {/* Tab Navigation */}
      <div className="bg-gray-800 border-b border-gray-700">
        <div className="container mx-auto px-4">
          <nav className="flex space-x-4">
            {tabs.map((tab) => (
              <button
                key={tab.id}
                onClick={() => tab.enabled && setActiveTab(tab.id)}
                className={`py-3 px-4 font-medium transition-colors ${
                  activeTab === tab.id
                    ? "text-blue-400 border-b-2 border-blue-400"
                    : tab.enabled
                    ? "text-gray-400 hover:text-gray-200"
                    : "text-gray-600 cursor-not-allowed"
                }`}
                disabled={!tab.enabled}
                title={!tab.enabled ? "Save a profile first to access this tab" : ""}
              >
                {tab.label}
              </button>
            ))}
          </nav>
        </div>
      </div>

      {/* Main Content */}
      <main className="container mx-auto px-4 py-8">
        {activeTab === "fancontrol" && (
          <div className="space-y-6">
            <div className="flex items-center justify-between">
              <h2 className="text-xl font-semibold">Fan Control</h2>
              <div className="flex items-center gap-2">
                <button
                  onClick={fetchHwmon}
                  className="inline-flex items-center gap-2 rounded-md bg-gray-700 hover:bg-gray-600 text-white px-3 py-2 text-sm"
                  title="Refresh sensors"
                >
                  Refresh
                </button>
              </div>
            </div>

            {/* Empty state before autodetect */}
            {(!mappings || mappings.length === 0) ? (
              <div className="rounded-2xl border border-white/10 bg-gray-800/70 p-8 text-center">
                <div className="mx-auto mb-4 flex h-12 w-12 items-center justify-center rounded-xl bg-blue-500/20 text-blue-400">
                  <svg className="h-6 w-6" viewBox="0 0 24 24" fill="none" xmlns="http://www.w3.org/2000/svg">
                    <path d="M12 3v3m0 12v3m8-8h-3M7 12H4m11.95-4.95l-2.12 2.12M8.17 15.83l-2.12 2.12M15.83 15.83l2.12 2.12M6.05 6.05l2.12 2.12" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" />
                  </svg>
                </div>
                <h3 className="text-lg font-semibold mb-1">No fans mapped yet</h3>
                <p className="text-gray-400 mb-6">Try autodetecting fan ↔ PWM mappings to get started. You can adjust later.</p>
                <button
                  onClick={() => setShowAutoDetectModal(true)}
                  className="inline-flex items-center gap-2 rounded-md bg-blue-600/90 hover:bg-blue-500 text-white px-4 py-2 text-sm"
                >
                  <svg className="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M9.663 17h4.673M12 3v1m6.364 1.636l-.707.707M21 12h-1M4 12H3m3.343-5.657l-.707-.707m2.828 9.9a5 5 0 117.072 0l-.548.547A3.374 3.374 0 0014 18.469V19a2 2 0 11-4 0v-.531c0-.895-.356-1.754-.988-2.386l-.548-.547z" />
                  </svg>
                  Auto-Detect Mappings
                </button>
              </div>
            ) : (
              <div className="space-y-6">
                {/* Show detected mappings */}
                <div className="rounded-xl border border-white/10 bg-gray-900/40 p-4">
                  <h3 className="font-medium mb-3">Detected Fan ↔ PWM</h3>
                  <div className="grid grid-cols-1 md:grid-cols-2 gap-3">
                    {mappings.map((m, idx) => (
                      <div key={idx} className="flex items-center justify-between rounded-lg border border-white/10 bg-gray-800/60 p-3">
                        <div className="text-sm">
                          <div className="text-gray-300">{m.fan_name}</div>
                          <div className="text-gray-500">→ {m.pwm_name}</div>
                        </div>
                        <div className="text-xs text-gray-400">conf {Math.round(m.confidence * 100)}%</div>
                      </div>
                    ))}
                  </div>
                </div>

                {/* Fan and PWM cards */}
                <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
                  {(chips ?? []).map((chip) => (
                    (chip.fans.length > 0 || chip.pwms.length > 0) && (
                      <div key={chip.path} className="rounded-xl border border-white/10 bg-gray-900/50 p-4">
                        <div className="mb-2 flex items-center justify-between">
                          <div className="font-semibold text-white">{chip.name}</div>
                          <div className="text-xs text-gray-500 truncate max-w-[12rem]" title={chip.path}>{chip.path.split("/").pop()}</div>
                        </div>
                        <div className="space-y-3">
                          {/* Fans */}
                          {chip.fans.length > 0 && (
                            <div>
                              <div className="text-xs uppercase tracking-wide text-gray-400 mb-1">Fans</div>
                              <div className="space-y-2">
                                {chip.fans.map((f) => (
                                  <div key={f.input_path} className="flex items-center justify-between rounded-lg border border-white/10 bg-gray-800/40 p-2">
                                    <div className="text-sm text-gray-300">{f.label || f.name}</div>
                                    <div className="text-sm text-white">{f.current_rpm != null ? `${f.current_rpm} RPM` : "—"}</div>
                                  </div>
                                ))}
                              </div>
                            </div>
                          )}

                          {/* PWMs */}
                          {chip.pwms.length > 0 && (
                            <div>
                              <div className="text-xs uppercase tracking-wide text-gray-400 mb-1">PWM Controllers</div>
                              <div className="space-y-2">
                                {chip.pwms.map((p) => (
                                  <div key={p.pwm_path} className="flex items-center justify-between rounded-lg border border-white/10 bg-gray-800/40 p-2">
                                    <div className="text-sm text-gray-300">{p.label || p.name}</div>
                                    <div className="text-sm text-white">{p.current_percent != null ? `${p.current_percent.toFixed(0)}%` : (p.current_value != null ? `${Math.round((p.current_value / 255) * 100)}%` : "—")}</div>
                                  </div>
                                ))}
                              </div>
                            </div>
                          )}
                        </div>
                      </div>
                    )
                  )).filter(Boolean)}
                </div>
              </div>
            )}
          </div>
        )}

        {activeTab === "sensors" && (
          <div className="space-y-6">
            <div className="flex items-center justify-between">
              <h2 className="text-xl font-semibold">Sensors</h2>
              <div className="flex items-center gap-2">
                <button
                  onClick={() => { fetchHwmon(); fetchSensorNames(); }}
                  className="inline-flex items-center gap-2 rounded-md bg-gray-700 hover:bg-gray-600 text-white px-3 py-2 text-sm"
                  title="Refresh"
                >
                  Refresh
                </button>
              </div>
            </div>

            {/* Temperature sensor cards */}
            <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
              {(chips ?? []).flatMap((chip) => (
                chip.temperatures.map((t) => {
                  const key = t.input_path; // unique path key
                  const currentName = sensorNames[key] ?? "";
                  return (
                    <div key={key} className="rounded-xl border border-white/10 bg-gray-900/50 p-4">
                      <div className="mb-3">
                        <div className="text-xs text-gray-500 mb-1">{chip.name}</div>
                        <div className="text-base font-semibold text-white flex items-center justify-between">
                          <span>{currentName || t.label || t.name}</span>
                          <span className="text-sm text-blue-300">{t.current_temp != null ? `${t.current_temp.toFixed(1)}°C` : "—"}</span>
                        </div>
                      </div>
                      <div className="space-y-2">
                        <label className="block text-xs text-gray-400">Custom name</label>
                        <div className="flex gap-2">
                          <input
                            defaultValue={currentName}
                            placeholder="e.g. CPU Package"
                            onBlur={async (e) => {
                              const val = e.currentTarget.value.trim();
                              try {
                                await invoke("set_sensor_name_cmd", { keyInputPath: key, name: val });
                                await fetchSensorNames();
                              } catch (err) { console.error(err); }
                            }}
                            className="flex-1 rounded-md bg-gray-800/70 border border-white/10 px-3 py-2 text-sm text-white placeholder-gray-500 focus:outline-none focus:ring-1 focus:ring-blue-500"
                          />
                          <button
                            onClick={async () => { try { await invoke("set_sensor_name_cmd", { keyInputPath: key, name: "" }); await fetchSensorNames(); } catch (err) { console.error(err); } }}
                            className="rounded-md bg-gray-700 hover:bg-gray-600 text-white px-3 py-2 text-sm"
                            title="Clear custom name"
                          >
                            Clear
                          </button>
                        </div>
                        <div className="text-[10px] text-gray-500 truncate" title={key}>{key}</div>
                      </div>
                    </div>
                  );
                })
              ))}
            </div>
          </div>
        )}


        {activeTab === "profiles" && (
          <div className="space-y-6">
            <h2 className="text-xl font-semibold mb-4">Profile Manager</h2>
            <div className="bg-gray-800 rounded-lg p-6">
              <p className="text-gray-400">Profile management interface coming soon...</p>
            </div>
          </div>
        )}
      </main>

      {/* Auto-Detect Modal */}
      <AutoDetectModal
        isOpen={showAutoDetectModal}
        onClose={() => setShowAutoDetectModal(false)}
        onConfirm={handleAutoDetectConfirm}
      />
    </div>
  );
}

export default App;
