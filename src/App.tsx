import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  CheckCircle2,
  ClipboardCopy,
  Download,
  ExternalLink,
  FolderOpen,
  Gamepad2,
  HardDriveDownload,
  Layers3,
  Loader2,
  LogIn,
  LogOut,
  PackageSearch,
  Play,
  RefreshCw,
  Search,
  Settings,
  ShieldCheck,
  TriangleAlert,
  Trash2,
  UserRound,
  X
} from "lucide-react";
import { useEffect, useMemo, useState } from "react";

type Tab = "play" | "versions" | "mods" | "account" | "settings";
type VersionFilter = "release" | "snapshot" | "all";
type ModProjectType = "mod" | "shader" | "resourcepack" | "modpack";

interface VersionEntry {
  id: string;
  type: string;
  kind?: string;
  url: string;
  time: string;
  release_time: string;
  releaseTime?: string;
  sha1: string;
  compliance_level?: number;
  complianceLevel?: number;
}

interface VersionManifest {
  latest: {
    release: string;
    snapshot: string;
  };
  versions: VersionEntry[];
}

interface InstalledVersion {
  id: string;
  display_name: string;
  kind: string;
  loader: string;
  has_client: boolean;
  has_manifest: boolean;
  path: string;
  inherits_from?: string;
  jar?: string;
}

interface DownloadProgress {
  phase: string;
  current: number;
  total: number;
  label: string;
}

interface LaunchLogEvent {
  stream: "info" | "command" | "stdout" | "stderr" | "exit" | string;
  line: string;
  pid?: number;
}

interface VersionSummary {
  id: string;
  main_class: string;
  asset_index: string;
  java_component?: string;
  java_major?: number;
  libraries: number;
  assets?: number;
  client_size?: number;
  game_arguments: number;
  jvm_arguments: number;
}

interface DeviceCodeResponse {
  device_code: string;
  user_code: string;
  verification_uri: string;
  expires_in: number;
  interval: number;
  message?: string;
  browser_opened?: boolean;
  browser_open_error?: string;
}

interface MinecraftProfile {
  id: string;
  name: string;
  access_token: string;
  refresh_token?: string;
  xuid?: string;
  owns_game: boolean;
  expires_in: number;
  expires_at?: number;
  account_type: "microsoft" | "offline" | string;
}

interface LoginPollResult {
  status: string;
  message: string;
  profile?: MinecraftProfile;
}

interface DataPaths {
  launcher_root: string;
  minecraft_root: string;
  versions_root: string;
  instances_root: string;
}

interface JavaInstallation {
  path: string;
  folder: string;
  version: string;
  major: number;
  is_jdk: boolean;
  is_64_bit: boolean;
  source: string;
  display_name: string;
}

interface ModrinthHit {
  project_id: string;
  slug: string;
  title: string;
  description: string;
  icon_url?: string;
  downloads: number;
  follows: number;
  categories?: string[];
  versions?: string[];
}

interface ModrinthSearchResponse {
  hits: ModrinthHit[];
  total_hits: number;
}

interface LoaderVersionOption {
  loader: string;
  version: string;
  display_name: string;
  recommended: boolean;
  stable: boolean;
}

interface ModrinthFile {
  filename: string;
  size?: number;
  primary?: boolean;
}

interface ModrinthVersion {
  id: string;
  name: string;
  version_number: string;
  game_versions?: string[];
  loaders?: string[];
  date_published?: string;
  downloads?: number;
  files?: ModrinthFile[];
}

interface ModVersionDialog {
  project: ModrinthHit;
  projectType: ModProjectType;
  versions: ModrinthVersion[];
  selectedVersionId: string;
  defaultTarget: string;
}

interface ModInstallResult {
  project_id: string;
  version_id: string;
  file_name: string;
  path: string;
  project_type?: string;
}

interface MemoryRecommendation {
  total_mb: number;
  available_mb: number;
  recommended_mb: number;
  mod_count: number;
  modable: boolean;
  reason: string;
}

interface SettingsState {
  javaMode: "auto" | "manual";
  javaPath: string;
  memoryMode: "auto" | "manual";
  maxMemoryMb: number;
  loader: string;
  downloadLoader: "none" | "fabric" | "forge";
  downloadLoaderVersion: string;
  lastVersionId?: string;
}

const DEFAULT_SETTINGS: SettingsState = {
  javaMode: "auto",
  javaPath: "",
  memoryMode: "auto",
  maxMemoryMb: 4096,
  loader: "fabric",
  downloadLoader: "none",
  downloadLoaderVersion: ""
};

const VERSION_MANIFEST_URL = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";
const MODRINTH_API = "https://api.modrinth.com/v2";
const BRAND_LOGO_CANDIDATES = ["/logo.png", "/logo.jpg", "/logo.jpeg", "/logo.bmp", "/logo.svg", "/logo.webp", "/logo.ico"];

declare global {
  interface Window {
    __TAURI_INTERNALS__?: unknown;
  }
}

function isTauriRuntime() {
  return Boolean(window.__TAURI_INTERNALS__);
}

const tabs: Array<{ id: Tab; label: string; icon: typeof Play }> = [
  { id: "play", label: "启动", icon: Play },
  { id: "versions", label: "版本", icon: Layers3 },
  { id: "mods", label: "模组", icon: PackageSearch },
  { id: "account", label: "账号", icon: UserRound },
  { id: "settings", label: "设置", icon: Settings }
];

function readSettings(): SettingsState {
  try {
    const stored = window.localStorage.getItem("coral-launcher-settings");
    if (!stored) return DEFAULT_SETTINGS;
    const parsed = JSON.parse(stored);
    return {
      ...DEFAULT_SETTINGS,
      ...parsed,
      javaMode: parsed.javaMode === "manual" ? "manual" : "auto",
      javaPath: typeof parsed.javaPath === "string" ? parsed.javaPath : "",
      memoryMode: parsed.memoryMode === "manual" ? "manual" : "auto",
      downloadLoader: ["fabric", "forge", "none"].includes(parsed.downloadLoader) ? parsed.downloadLoader : "none",
      downloadLoaderVersion: typeof parsed.downloadLoaderVersion === "string" ? parsed.downloadLoaderVersion : ""
    };
  } catch {
    return DEFAULT_SETTINGS;
  }
}

function saveSettings(settings: SettingsState) {
  window.localStorage.setItem("coral-launcher-settings", JSON.stringify(settings));
}

function formatBytes(bytes?: number) {
  if (!bytes) return "未知";
  const units = ["B", "KB", "MB", "GB"];
  let value = bytes;
  let unit = 0;
  while (value > 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value.toFixed(unit === 0 ? 0 : 1)} ${units[unit]}`;
}

function compactNumber(value: number) {
  return new Intl.NumberFormat("zh-CN", { notation: "compact" }).format(value);
}

function modProjectLabel(type: ModProjectType) {
  return type === "shader" ? "光影" : type === "resourcepack" ? "资源包" : type === "modpack" ? "整合包" : "模组";
}

function modProjectTarget(type: ModProjectType) {
  return type === "shader" ? "shaderpacks" : type === "resourcepack" ? "resourcepacks" : type === "modpack" ? "当前版本目录" : "mods";
}

function downloadLoaderLabel(loader: SettingsState["downloadLoader"]) {
  return loader === "fabric" ? "Fabric" : loader === "forge" ? "Forge" : "原版";
}

function modrinthVersionMeta(version?: ModrinthVersion) {
  if (!version) return "未选择版本";
  const gameVersions = version.game_versions?.slice(0, 4).join(", ") || "未知游戏版本";
  const loaders = version.loaders?.length ? ` · ${version.loaders.join(", ")}` : "";
  const file = version.files?.find((item) => item.primary) ?? version.files?.[0];
  return `${gameVersions}${loaders}${file ? ` · ${file.filename}` : ""}`;
}

function formatProfileExpiry(profile: MinecraftProfile) {
  if (profile.account_type === "offline") return "离线账号";
  const expiresAt = profile.expires_at ? profile.expires_at * 1000 : Date.now() + profile.expires_in * 1000;
  const remainingMs = expiresAt - Date.now();
  if (remainingMs <= 0) return "需要刷新";
  const remainingHours = Math.max(1, Math.round(remainingMs / 3_600_000));
  return `${remainingHours} 小时内`;
}

function versionType(version?: VersionEntry) {
  return version?.kind ?? version?.type ?? "release";
}

function versionReleaseTime(version: VersionEntry) {
  return version.release_time ?? version.releaseTime ?? version.time;
}

function formatLaunchLogEvent(event: LaunchLogEvent) {
  const label =
    event.stream === "stdout"
      ? "OUT"
      : event.stream === "stderr"
        ? "ERR"
        : event.stream === "command"
          ? "CMD"
          : event.stream === "exit"
            ? "EXIT"
            : "INFO";
  const pid = event.pid ? ` PID ${event.pid}` : "";
  return `[${new Date().toLocaleTimeString("zh-CN")}] [${label}${pid}] ${event.line}\n`;
}

function appendLaunchLog(current: string, next: string) {
  const merged = current + next;
  const limit = 240_000;
  if (merged.length <= limit) return merged;
  return `...前面的日志已自动截断，保留最近 ${Math.round(limit / 1024)} KB...\n${merged.slice(-limit)}`;
}

function App() {
  const [activeTab, setActiveTab] = useState<Tab>("play");
  const [manifest, setManifest] = useState<VersionManifest | null>(null);
  const [installed, setInstalled] = useState<InstalledVersion[]>([]);
  const [paths, setPaths] = useState<DataPaths | null>(null);
  const [selectedVersion, setSelectedVersion] = useState("");
  const [versionFilter, setVersionFilter] = useState<VersionFilter>("release");
  const [versionQuery, setVersionQuery] = useState("");
  const [summary, setSummary] = useState<VersionSummary | null>(null);
  const [progress, setProgress] = useState<DownloadProgress | null>(null);
  const [busy, setBusy] = useState("");
  const [status, setStatus] = useState("正在读取启动器数据");
  const [settings, setSettings] = useState<SettingsState>(() => readSettings());
  const [deviceCode, setDeviceCode] = useState<DeviceCodeResponse | null>(null);
  const [profile, setProfile] = useState<MinecraftProfile | null>(null);
  const [javaInstallations, setJavaInstallations] = useState<JavaInstallation[]>([]);
  const [offlineName, setOfflineName] = useState("Player");
  const [brandLogoIndex, setBrandLogoIndex] = useState(0);
  const [loaderVersions, setLoaderVersions] = useState<LoaderVersionOption[]>([]);
  const [modQuery, setModQuery] = useState("");
  const [modProjectType, setModProjectType] = useState<ModProjectType>("mod");
  const [modGameVersion, setModGameVersion] = useState("");
  const [modResults, setModResults] = useState<ModrinthHit[]>([]);
  const [modTotal, setModTotal] = useState(0);
  const [modVersionDialog, setModVersionDialog] = useState<ModVersionDialog | null>(null);
  const [lastInstall, setLastInstall] = useState<ModInstallResult | null>(null);
  const [memoryRecommendation, setMemoryRecommendation] = useState<MemoryRecommendation | null>(null);
  const [launchPreview, setLaunchPreview] = useState("");
  const [launchLogOpen, setLaunchLogOpen] = useState(false);
  const [launchLog, setLaunchLog] = useState("");

  useEffect(() => {
    saveSettings(settings);
  }, [settings]);

  useEffect(() => {
    if (!selectedVersion) return;
    setSettings((current) =>
      current.lastVersionId === selectedVersion ? current : { ...current, lastVersionId: selectedVersion }
    );
  }, [selectedVersion]);

  useEffect(() => {
    refreshMemoryRecommendation(selectedVersion, true);
  }, [selectedVersion, installed]);

  useEffect(() => {
    if (!selectedVersion || settings.downloadLoader === "none") {
      setLoaderVersions([]);
      if (settings.downloadLoaderVersion) {
        setSettings((current) => ({ ...current, downloadLoaderVersion: "" }));
      }
      return;
    }
    let cancelled = false;
    setLoaderVersions([]);
    if (!isTauriRuntime()) return;
    invoke<LoaderVersionOption[]>("get_loader_versions", {
      gameVersion: selectedVersion,
      loader: settings.downloadLoader
    })
      .then((options) => {
        if (cancelled) return;
        setLoaderVersions(options);
        setSettings((current) => {
          if (current.downloadLoader !== settings.downloadLoader) return current;
          if (options.some((item) => item.version === current.downloadLoaderVersion)) return current;
          return { ...current, downloadLoaderVersion: options[0]?.version ?? "" };
        });
      })
      .catch((error) => {
        if (!cancelled) {
          setStatus(`加载器版本读取失败：${String(error)}`);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [selectedVersion, settings.downloadLoader]);

  useEffect(() => {
    if (!isTauriRuntime()) return;
    const unlistenDownload = listen<DownloadProgress>("download-progress", (event) => {
      setProgress(event.payload);
    });
    const unlistenLaunch = listen<LaunchLogEvent>("launch-log", (event) => {
      setLaunchLogOpen(true);
      setLaunchLog((current) => appendLaunchLog(current, formatLaunchLogEvent(event.payload)));
    });
    return () => {
      unlistenDownload.then((unlisten) => unlisten());
      unlistenLaunch.then((unlisten) => unlisten());
    };
  }, []);

  useEffect(() => {
    refreshAll();
  }, []);

  useEffect(() => {
    loadSavedProfile();
  }, []);

  useEffect(() => {
    refreshJavaInstallations(true);
  }, []);

  useEffect(() => {
    if (!deviceCode || profile) return;
    const interval = window.setInterval(() => {
      pollLogin();
    }, Math.max(deviceCode.interval, 5) * 1000);
    return () => window.clearInterval(interval);
  }, [deviceCode, profile]);

  const selectedEntry = useMemo(
    () => manifest?.versions.find((item) => item.id === selectedVersion),
    [manifest, selectedVersion]
  );

  const selectedInstalledInfo = useMemo(
    () => installed.find((item) => item.id === selectedVersion),
    [installed, selectedVersion]
  );

  const selectedInstalled = useMemo(
    () => Boolean(selectedInstalledInfo?.has_client && selectedInstalledInfo?.has_manifest),
    [selectedInstalledInfo]
  );

  useEffect(() => {
    const baseVersion = selectedInstalledInfo?.inherits_from || selectedVersion;
    setModGameVersion(baseVersion || "");
  }, [selectedInstalledInfo?.inherits_from, selectedVersion]);

  const selectedJavaInfo = useMemo(
    () => javaInstallations.find((java) => java.path === settings.javaPath),
    [javaInstallations, settings.javaPath]
  );

  const javaModeLabel = useMemo(() => {
    if (settings.javaMode === "auto") return "自动选择";
    return selectedJavaInfo?.display_name || settings.javaPath || "未选择";
  }, [selectedJavaInfo, settings.javaMode, settings.javaPath]);

  const memoryLabel = useMemo(() => {
    if (settings.memoryMode === "manual") return `${settings.maxMemoryMb} MB`;
    return memoryRecommendation
      ? `自动 ${memoryRecommendation.recommended_mb} MB`
      : selectedInstalled
        ? "自动计算中"
        : "自动";
  }, [memoryRecommendation, selectedInstalled, settings.maxMemoryMb, settings.memoryMode]);

  const visibleInstalled = useMemo(() => {
    const query = versionQuery.trim().toLowerCase();
    return installed.filter((version) => {
      const text = `${version.id} ${version.display_name} ${version.loader}`.toLowerCase();
      return !query || text.includes(query);
    });
  }, [installed, versionQuery]);

  const visibleVersions = useMemo(() => {
    const query = versionQuery.trim().toLowerCase();
    return (
      manifest?.versions
        .filter((version) => versionFilter === "all" || versionType(version) === versionFilter)
        .filter((version) => !query || version.id.toLowerCase().includes(query))
        .slice(0, 120) ?? []
    );
  }, [manifest, versionFilter, versionQuery]);

  async function refreshAll() {
    setBusy("refresh");
    try {
      const [manifestData, installedData, pathData] = isTauriRuntime()
        ? await Promise.all([
            invoke<VersionManifest>("get_version_manifest"),
            invoke<InstalledVersion[]>("list_installed_versions"),
            invoke<DataPaths>("get_data_paths")
          ])
        : await Promise.all([
            fetch(VERSION_MANIFEST_URL).then((response) => response.json() as Promise<VersionManifest>),
            Promise.resolve([] as InstalledVersion[]),
            Promise.resolve(null as DataPaths | null)
          ]);
      setManifest(manifestData);
      setInstalled(installedData);
      setPaths(pathData);
      setSelectedVersion((current) => {
        if (current && (installedData.some((item) => item.id === current) || manifestData.versions.some((item) => item.id === current))) {
          return current;
        }
        if (
          settings.lastVersionId &&
          (installedData.some((item) => item.id === settings.lastVersionId) || manifestData.versions.some((item) => item.id === settings.lastVersionId))
        ) {
          return settings.lastVersionId;
        }
        return installedData[0]?.id ?? manifestData.latest.release;
      });
      setStatus(
        isTauriRuntime()
          ? `已同步 Mojang 版本清单，最新正式版 ${manifestData.latest.release}`
          : `浏览器预览模式，最新正式版 ${manifestData.latest.release}`
      );
    } catch (error) {
      setStatus(`读取失败：${String(error)}`);
    } finally {
      setBusy("");
    }
  }

  async function refreshInstalled() {
    if (!isTauriRuntime()) return;
    const installedData = await invoke<InstalledVersion[]>("list_installed_versions");
    setInstalled(installedData);
    setSelectedVersion((current) => {
      if (current && installedData.some((item) => item.id === current)) return current;
      if (settings.lastVersionId && installedData.some((item) => item.id === settings.lastVersionId)) return settings.lastVersionId;
      return installedData[0]?.id || manifest?.latest.release || "";
    });
  }

  async function chooseMinecraftRoot() {
    if (!isTauriRuntime()) {
      setStatus("选择 .minecraft 目录需要在 Tauri 桌面窗口中运行");
      return;
    }
    setBusy("choose-root");
    try {
      const selectedPaths = await invoke<DataPaths | null>("choose_minecraft_root");
      if (!selectedPaths) {
        setStatus("已取消选择 .minecraft 文件夹");
        return;
      }
      const installedData = await invoke<InstalledVersion[]>("list_installed_versions");
      setPaths(selectedPaths);
      setInstalled(installedData);
      setSelectedVersion((current) => {
        if (current && installedData.some((item) => item.id === current)) return current;
        if (settings.lastVersionId && installedData.some((item) => item.id === settings.lastVersionId)) return settings.lastVersionId;
        return installedData[0]?.id ?? manifest?.latest.release ?? "";
      });
      setSummary(null);
      setLaunchPreview("");
      setStatus(`已切换 .minecraft：${selectedPaths.minecraft_root}，识别到 ${installedData.length} 个本地版本`);
    } catch (error) {
      setStatus(`选择 .minecraft 失败：${String(error)}`);
    } finally {
      setBusy("");
    }
  }

  async function refreshJavaInstallations(silent = false) {
    if (!isTauriRuntime()) return;
    if (!silent) setBusy("java-scan");
    try {
      const javaList = await invoke<JavaInstallation[]>("scan_java_installations");
      setJavaInstallations(javaList);
      if (!silent) setStatus(`已找到 ${javaList.length} 个可用 Java`);
    } catch (error) {
      if (!silent) setStatus(`搜索 Java 失败：${String(error)}`);
    } finally {
      if (!silent) setBusy("");
    }
  }

  async function refreshMemoryRecommendation(versionId = selectedVersion, silent = false) {
    if (!isTauriRuntime() || !versionId || !installed.some((item) => item.id === versionId)) {
      setMemoryRecommendation(null);
      return;
    }
    if (!silent) setBusy("memory");
    try {
      const recommendation = await invoke<MemoryRecommendation>("recommend_memory", { versionId });
      setMemoryRecommendation(recommendation);
      if (!silent) setStatus(`自动内存建议：${recommendation.recommended_mb} MB`);
    } catch (error) {
      setMemoryRecommendation(null);
      if (!silent) setStatus(`计算自动内存失败：${String(error)}`);
    } finally {
      if (!silent) setBusy("");
    }
  }

  async function chooseJavaExecutable() {
    if (!isTauriRuntime()) {
      setStatus("导入 Java 需要在 Tauri 桌面窗口中运行");
      return;
    }
    setBusy("java-choose");
    try {
      const selectedJava = await invoke<JavaInstallation | null>("choose_java_executable");
      if (!selectedJava) {
        setStatus("已取消导入 Java");
        return;
      }
      setJavaInstallations((current) => {
        if (current.some((java) => java.path === selectedJava.path)) return current;
        return [...current, selectedJava].sort((left, right) => left.major - right.major || left.path.localeCompare(right.path));
      });
      setSettings((current) => ({ ...current, javaMode: "manual", javaPath: selectedJava.path }));
      setStatus(`已选择 ${selectedJava.display_name}`);
    } catch (error) {
      setStatus(`导入 Java 失败：${String(error)}`);
    } finally {
      setBusy("");
    }
  }

  async function loadSavedProfile() {
    if (!isTauriRuntime()) return;
    try {
      const savedProfile = await invoke<MinecraftProfile | null>("get_saved_minecraft_profile");
      if (!savedProfile) return;
      setProfile(savedProfile);
      const expiresSoon = savedProfile.expires_at ? savedProfile.expires_at * 1000 < Date.now() + 10 * 60 * 1000 : false;
      if (expiresSoon && savedProfile.account_type !== "offline") {
        await refreshProfile(true);
        return;
      }
      setStatus(`${savedProfile.name} 的${savedProfile.account_type === "offline" ? "离线账号" : "正版登录"}状态已恢复`);
    } catch (error) {
      setStatus(`读取登录状态失败：${String(error)}`);
    }
  }

  async function refreshProfile(silent = false) {
    if (!isTauriRuntime()) {
      if (!silent) setStatus("刷新登录需要在 Tauri 桌面窗口中运行");
      return;
    }
    if (!silent) setBusy("refresh-login");
    try {
      const refreshed = await invoke<MinecraftProfile>("refresh_minecraft_profile");
      setProfile(refreshed);
      setStatus(`${refreshed.name} 的正版登录已刷新`);
    } catch (error) {
      setStatus(silent ? `已恢复本地账号，但自动刷新失败：${String(error)}` : `刷新登录失败：${String(error)}`);
    } finally {
      if (!silent) setBusy("");
    }
  }

  async function logoutProfile() {
    setBusy("logout");
    try {
      if (isTauriRuntime()) {
        await invoke("logout_minecraft_profile");
      }
      setProfile(null);
      setDeviceCode(null);
      setStatus("已退出正版账号");
    } catch (error) {
      setStatus(`退出登录失败：${String(error)}`);
    } finally {
      setBusy("");
    }
  }

  async function createOfflineAccount() {
    if (!isTauriRuntime()) {
      setStatus("离线账号需要在 Tauri 桌面窗口中创建");
      return;
    }
    setBusy("offline-login");
    try {
      const offlineProfile = await invoke<MinecraftProfile>("create_offline_profile", {
        request: { name: offlineName }
      });
      setProfile(offlineProfile);
      setDeviceCode(null);
      setStatus(`${offlineProfile.name} 的离线账号已启用`);
    } catch (error) {
      setStatus(`创建离线账号失败：${String(error)}`);
    } finally {
      setBusy("");
    }
  }

  async function inspectVersion(versionId = selectedVersion) {
    if (!versionId) return;
    setBusy("summary");
    try {
      if (isTauriRuntime()) {
        setSummary(await invoke<VersionSummary>("get_version_summary", { versionId }));
      } else {
        const entry = manifest?.versions.find((version) => version.id === versionId);
        if (!entry) throw new Error("未找到版本");
        const versionJson = await fetch(entry.url).then((response) => response.json());
        setSummary({
          id: versionJson.id,
          main_class: versionJson.mainClass,
          asset_index: versionJson.assets ?? versionJson.assetIndex?.id ?? "",
          java_component: versionJson.javaVersion?.component,
          java_major: versionJson.javaVersion?.majorVersion,
          libraries: versionJson.libraries?.length ?? 0,
          assets: undefined,
          client_size: versionJson.downloads?.client?.size,
          game_arguments: versionJson.arguments?.game?.length ?? versionJson.minecraftArguments?.split(" ").length ?? 0,
          jvm_arguments: versionJson.arguments?.jvm?.length ?? 0
        });
      }
      setStatus(`已读取 ${versionId} 的启动元数据`);
    } catch (error) {
      setStatus(`版本解析失败：${String(error)}`);
    } finally {
      setBusy("");
    }
  }

  async function downloadSelected() {
    if (!selectedVersion) return;
    if (!isTauriRuntime()) {
      setStatus("下载需要在 Tauri 桌面窗口中运行");
      return;
    }
    setBusy("download");
    setProgress({ phase: "queued", current: 0, total: 1, label: "准备下载" });
    try {
      await invoke("download_version", {
        versionId: selectedVersion,
        includeAssets: true,
        loader: settings.downloadLoader,
        loaderVersion: settings.downloadLoaderVersion
      });
      await refreshInstalled();
      setStatus(
        settings.downloadLoader === "none"
          ? `${selectedVersion} 已下载完成`
          : `${selectedVersion} 已下载完成，并附加安装 ${downloadLoaderLabel(settings.downloadLoader)} ${settings.downloadLoaderVersion || "最新版本"}`
      );
    } catch (error) {
      setStatus(`下载失败：${String(error)}`);
    } finally {
      setBusy("");
    }
  }

  async function deleteSelectedVersion() {
    if (!selectedInstalledInfo) {
      setStatus("请选择一个已安装的本地版本");
      return;
    }
    if (!isTauriRuntime()) {
      setStatus("删除版本需要在 Tauri 桌面窗口中运行");
      return;
    }
    const confirmed = window.confirm(`确认删除版本 ${selectedInstalledInfo.id}？这会移除对应 versions 文件夹。`);
    if (!confirmed) return;
    setBusy("delete-version");
    try {
      await invoke("delete_installed_version", { versionId: selectedInstalledInfo.id });
      setSummary(null);
      setLaunchPreview("");
      await refreshInstalled();
      setStatus(`已删除版本 ${selectedInstalledInfo.id}`);
    } catch (error) {
      setStatus(`删除版本失败：${String(error)}`);
    } finally {
      setBusy("");
    }
  }

  async function beginLogin() {
    if (!isTauriRuntime()) {
      setStatus("正版验证需要在 Tauri 桌面窗口中运行");
      return;
    }
    setBusy("login");
    try {
      const response = await invoke<DeviceCodeResponse>("begin_microsoft_device_login");
      setDeviceCode(response);
      setLaunchPreview("");
      setStatus(
        response.browser_opened
          ? "登录码已生成，Microsoft 登录页已在浏览器打开"
          : `登录码已生成，但浏览器未能自动打开：${response.browser_open_error ?? "未知原因"}`
      );
    } catch (error) {
      setStatus(`登录启动失败：${String(error)}`);
    } finally {
      setBusy("");
    }
  }

  async function pollLogin() {
    if (!deviceCode) return;
    if (!isTauriRuntime()) return;
    try {
      const result = await invoke<LoginPollResult>("poll_microsoft_device_login", {
        deviceCode: deviceCode.device_code
      });
      setStatus(result.message);
      if (result.status === "done" && result.profile) {
        setProfile(result.profile);
        setDeviceCode(null);
      }
    } catch (error) {
      setStatus(`正版验证失败：${String(error)}`);
      setDeviceCode(null);
    }
  }

  function launchAccount() {
    if (!profile) return null;
    return {
      name: profile.name,
      uuid: profile.id,
      access_token: profile.access_token,
      xuid: profile.xuid,
      owns_game: profile.owns_game,
      account_type: profile.account_type
    };
  }

  async function previewCommand() {
    if (!isTauriRuntime()) {
      setStatus("启动命令预览需要在 Tauri 桌面窗口中运行");
      return;
    }
    if (!selectedInstalled) {
      setStatus("请先下载当前选择的版本");
      return;
    }
    setBusy("preview");
    try {
      const result = await invoke<{ command_preview: string }>("preview_launch_command", {
        versionId: selectedVersion,
        javaPath: settings.javaMode === "manual" ? settings.javaPath : "auto",
        maxMemoryMb: settings.maxMemoryMb,
        memoryMode: settings.memoryMode,
        account: launchAccount()
      });
      setLaunchPreview(result.command_preview);
      setStatus("启动命令已生成");
    } catch (error) {
      setStatus(`生成启动命令失败：${String(error)}`);
    } finally {
      setBusy("");
    }
  }

  async function launchGame() {
    if (!isTauriRuntime()) {
      setStatus("启动游戏需要在 Tauri 桌面窗口中运行");
      return;
    }
    if (!selectedInstalled) {
      setStatus("请先下载当前选择的版本");
      return;
    }
    if (!profile) {
      setStatus("请先完成正版登录或创建离线账号");
      return;
    }
    if (profile.account_type !== "offline" && !profile.owns_game) {
      setStatus("该账号未检测到 Minecraft Java Edition 授权，无法启动正版会话");
      return;
    }
    setBusy("launch");
    setLaunchLogOpen(true);
    setLaunchLog(
      [
        "Coral Launcher 启动日志",
        `时间: ${new Date().toLocaleString("zh-CN")}`,
        `版本: ${selectedVersion}`,
        `.minecraft: ${paths?.minecraft_root ?? "未知"}`,
        `Java: ${settings.javaMode === "manual" ? settings.javaPath || "未选择" : "自动选择"}`,
        `内存: ${
          settings.memoryMode === "auto"
            ? `自动${memoryRecommendation ? ` (${memoryRecommendation.recommended_mb} MB)` : ""}`
            : `${settings.maxMemoryMb} MB`
        }`,
        ""
      ].join("\n")
    );
    try {
      const result = await invoke<{ pid?: number; game_directory: string }>("launch_game", {
        versionId: selectedVersion,
        javaPath: settings.javaMode === "manual" ? settings.javaPath : "auto",
        maxMemoryMb: settings.maxMemoryMb,
        memoryMode: settings.memoryMode,
        account: launchAccount()
      });
      setStatus(`游戏进程已启动：PID ${result.pid ?? "未知"}，目录 ${result.game_directory}`);
      setLaunchLog((current) =>
        appendLaunchLog(current, `[${new Date().toLocaleTimeString("zh-CN")}] [INFO] 前端收到启动结果：PID ${result.pid ?? "未知"}，目录 ${result.game_directory}\n`)
      );
    } catch (error) {
      setStatus(`启动失败：${String(error)}`);
      setLaunchLog((current) =>
        appendLaunchLog(current, `[${new Date().toLocaleTimeString("zh-CN")}] [ERROR] 启动失败：${String(error)}\n`)
      );
    } finally {
      setBusy("");
    }
  }

  async function copyLaunchLog() {
    try {
      await navigator.clipboard.writeText(launchLog);
      setStatus("启动日志已复制到剪贴板");
    } catch (error) {
      setStatus(`复制失败，请在日志框中手动复制：${String(error)}`);
    }
  }

  async function searchMods() {
    setBusy("mods");
    try {
      const targetGameVersion = modGameVersion || selectedInstalledInfo?.inherits_from || selectedVersion;
      const response = isTauriRuntime()
        ? await invoke<ModrinthSearchResponse>("search_modrinth", {
            query: modQuery,
            gameVersion: targetGameVersion,
            loader: settings.loader,
            projectType: modProjectType,
            limit: 24
          })
        : await fetch(
            `${MODRINTH_API}/search?${new URLSearchParams({
              query: modQuery,
              facets: JSON.stringify([
                [`project_type:${modProjectType}`],
                ...(targetGameVersion ? [[`versions:${targetGameVersion}`]] : []),
                ...(settings.loader && (modProjectType === "mod" || modProjectType === "modpack") ? [[`categories:${settings.loader}`]] : [])
              ]),
              limit: "24",
              index: "relevance"
            })}`
          ).then((response) => response.json() as Promise<ModrinthSearchResponse>);
      setModResults(response.hits ?? []);
      setModTotal(response.total_hits ?? 0);
      setStatus(`Modrinth 返回 ${response.total_hits ?? 0} 个${modProjectLabel(modProjectType)}结果`);
    } catch (error) {
      setStatus(`模组搜索失败：${String(error)}`);
    } finally {
      setBusy("");
    }
  }

  async function openModVersionPicker(project: ModrinthHit) {
    if (!isTauriRuntime()) {
      setStatus(`${modProjectLabel(modProjectType)}安装需要在 Tauri 桌面窗口中运行`);
      return;
    }
    if (!selectedVersion) {
      setStatus("请先选择一个游戏版本");
      return;
    }
    setBusy(`mod-${project.project_id}`);
    try {
      const targetGameVersion = modGameVersion || selectedInstalledInfo?.inherits_from || selectedVersion;
      const [versions, defaultTarget] = await Promise.all([
        invoke<ModrinthVersion[]>("get_modrinth_project_versions", {
          projectId: project.project_id,
          gameVersion: targetGameVersion,
          loader: settings.loader,
          projectType: modProjectType
        }),
        invoke<string>("get_resource_default_target_folder", {
          gameVersion: selectedVersion,
          projectType: modProjectType
        })
      ]);
      if (!versions.length) {
        setStatus(`没有找到适配 ${targetGameVersion} 的${modProjectLabel(modProjectType)}版本`);
        return;
      }
      setModVersionDialog({
        project,
        projectType: modProjectType,
        versions,
        selectedVersionId: versions[0].id,
        defaultTarget
      });
      setStatus(`已读取 ${project.title} 的 ${versions.length} 个可用版本`);
    } catch (error) {
      setStatus(`读取${modProjectLabel(modProjectType)}版本失败：${String(error)}`);
    } finally {
      setBusy("");
    }
  }

  async function installSelectedModrinthVersion(useFolderPicker: boolean) {
    if (!modVersionDialog || !modVersionDialog.selectedVersionId) return;
    if (!isTauriRuntime()) {
      setStatus(`${modProjectLabel(modVersionDialog.projectType)}安装需要在 Tauri 桌面窗口中运行`);
      return;
    }
    setBusy(`mod-install-${modVersionDialog.project.project_id}`);
    try {
      let targetDir = modVersionDialog.defaultTarget;
      const targetGameVersion = modGameVersion || selectedInstalledInfo?.inherits_from || selectedVersion;
      if (useFolderPicker) {
        const picked = await invoke<string | null>("choose_resource_target_folder", {
          gameVersion: selectedVersion,
          projectType: modVersionDialog.projectType
        });
        if (!picked) {
          setStatus("已取消选择下载文件夹");
          return;
        }
        targetDir = picked;
      }
      const result = await invoke<ModInstallResult>("install_modrinth_project_version", {
        projectId: modVersionDialog.project.project_id,
        versionId: modVersionDialog.selectedVersionId,
        gameVersion: targetGameVersion,
        projectType: modVersionDialog.projectType,
        targetDir
      });
      setLastInstall(result);
      setStatus(`已安装 ${modProjectLabel(modVersionDialog.projectType)}：${result.file_name}`);
      setModVersionDialog(null);
    } catch (error) {
      setStatus(`${modProjectLabel(modVersionDialog.projectType)}安装失败：${String(error)}`);
    } finally {
      setBusy("");
    }
  }

  const latestRelease = manifest?.latest.release ?? "未知";
  const latestSnapshot = manifest?.latest.snapshot ?? "未知";
  const progressPercent = progress ? Math.round((progress.current / Math.max(progress.total, 1)) * 100) : 0;
  const selectedVersionLabel = selectedInstalledInfo?.display_name || selectedEntry?.id || selectedVersion || "未选择版本";
  const selectedVersionMeta = selectedInstalledInfo
    ? `${selectedInstalledInfo.loader} · ${selectedInstalledInfo.kind}${selectedInstalledInfo.inherits_from ? ` · 继承 ${selectedInstalledInfo.inherits_from}` : ""}`
    : selectedEntry
      ? `${versionType(selectedEntry)} · 尚未安装`
      : "请到版本页选择或下载";
  const selectedLoaderVersion = loaderVersions.find((item) => item.version === settings.downloadLoaderVersion);
  const selectedModrinthVersion = modVersionDialog?.versions.find((item) => item.id === modVersionDialog.selectedVersionId);
  const modVersionInstallBusy = modVersionDialog ? busy === `mod-install-${modVersionDialog.project.project_id}` : false;
  const targetModGameVersion = modGameVersion || selectedInstalledInfo?.inherits_from || selectedVersion || latestRelease;

  const downloadLoaderPicker = (
    <div className="loader-picker">
      <div className="loader-picker-heading">
        <span>下载时附加加载器</span>
        <strong>
          {settings.downloadLoader === "none"
            ? "仅原版"
            : `${downloadLoaderLabel(settings.downloadLoader)} ${selectedLoaderVersion?.version || settings.downloadLoaderVersion || "自动"}`}
        </strong>
      </div>
      <div className="segmented loader-segmented">
        {(["none", "fabric", "forge"] as SettingsState["downloadLoader"][]).map((loader) => (
          <button
            key={loader}
            className={settings.downloadLoader === loader ? "active" : ""}
            onClick={() =>
              setSettings((current) => ({
                ...current,
                downloadLoader: loader,
                downloadLoaderVersion: loader === "none" || current.downloadLoader !== loader ? "" : current.downloadLoaderVersion
              }))
            }
          >
            {loader === "none" ? "原版" : downloadLoaderLabel(loader)}
          </button>
        ))}
      </div>
      {settings.downloadLoader !== "none" && (
        <label className="input-label compact-label">
          加载器版本
          <select
            value={settings.downloadLoaderVersion}
            onChange={(event) => setSettings({ ...settings, downloadLoaderVersion: event.target.value })}
          >
            <option value="">自动选择最新可用版本</option>
            {loaderVersions.map((option) => (
              <option key={`${option.loader}-${option.version}`} value={option.version}>
                {option.display_name}
                {option.recommended ? " · 推荐" : ""}
              </option>
            ))}
          </select>
        </label>
      )}
    </div>
  );

  return (
    <div className="app-shell">
      <aside className="sidebar">
        <div className="brand">
          {brandLogoIndex < BRAND_LOGO_CANDIDATES.length ? (
            <img
              className="brand-logo"
              src={BRAND_LOGO_CANDIDATES[brandLogoIndex]}
              alt=""
              onError={() => setBrandLogoIndex((index) => index + 1)}
            />
          ) : (
            <div className="brand-mark">C</div>
          )}
          <div>
            <strong>Coral</strong>
            <span>Launcher</span>
          </div>
        </div>

        <nav className="nav-list">
          {tabs.map((tab) => {
            const Icon = tab.icon;
            return (
              <button
                key={tab.id}
                className={activeTab === tab.id ? "nav-item active" : "nav-item"}
                onClick={() => setActiveTab(tab.id)}
                title={tab.label}
              >
                <Icon size={18} />
                <span>{tab.label}</span>
              </button>
            );
          })}
        </nav>

        <div className="side-status">
          <div className="status-dot" />
          <p>{status}</p>
        </div>
      </aside>

      <main className="main">
        <header className="topbar">
          <div>
            <p className="eyebrow">Minecraft Java Edition</p>
            <h1>{selectedVersion || latestRelease}</h1>
          </div>
          <div className="top-actions">
            <button className="icon-button" onClick={refreshAll} title="刷新版本清单">
              {busy === "refresh" ? <Loader2 className="spin" size={18} /> : <RefreshCw size={18} />}
            </button>
            <button
              className={
                profile
                  ? profile.account_type === "offline"
                    ? "account-pill offline"
                    : profile.owns_game
                      ? "account-pill verified"
                      : "account-pill warning"
                  : "account-pill"
              }
              onClick={() => setActiveTab("account")}
              title="账号状态"
            >
              {profile ? (
                profile.account_type === "offline" ? (
                  <UserRound size={17} />
                ) : profile.owns_game ? (
                  <ShieldCheck size={17} />
                ) : (
                  <TriangleAlert size={17} />
                )
              ) : (
                <UserRound size={17} />
              )}
              <span>{profile ? profile.name : "未登录"}</span>
            </button>
          </div>
        </header>

        {activeTab === "play" && (
          <section className="page-grid play-grid">
            <div className="panel launch-panel">
              <div className="panel-heading">
                <div>
                  <p className="eyebrow">Play</p>
                  <h2>启动配置</h2>
                </div>
                <span className={selectedInstalled ? "badge ok" : "badge"}>{selectedInstalled ? "已安装" : "未安装"}</span>
              </div>

              <div className="selected-version-strip">
                <div>
                  <label>游戏版本</label>
                  <strong>{selectedVersionLabel}</strong>
                  <span>{selectedVersionMeta}</span>
                </div>
                <button className="secondary-action compact" onClick={() => setActiveTab("versions")}>
                  <Layers3 size={18} />
                  <span>选择版本</span>
                </button>
              </div>

              <div className="action-row">
                <button className="primary-action" onClick={launchGame} disabled={busy === "launch"}>
                  {busy === "launch" ? <Loader2 className="spin" size={18} /> : <Play size={18} />}
                  <span>启动游戏</span>
                </button>
                <button className="secondary-action" onClick={downloadSelected} disabled={busy === "download"}>
                  {busy === "download" ? <Loader2 className="spin" size={18} /> : <HardDriveDownload size={18} />}
                  <span>下载版本</span>
                </button>
                <button className="secondary-action" onClick={previewCommand} disabled={busy === "preview"}>
                  <Gamepad2 size={18} />
                  <span>预览命令</span>
                </button>
              </div>

              <div className="launch-facts">
                <div>
                  <span>Java</span>
                  <strong>{javaModeLabel}</strong>
                </div>
                <div>
                  <span>内存</span>
                  <strong>{memoryLabel}</strong>
                </div>
                <div>
                  <span>账号</span>
                  <strong>
                    {profile
                      ? profile.account_type === "offline"
                        ? "离线账号"
                        : profile.owns_game
                          ? "Microsoft 正版"
                          : "未授权"
                      : "未验证"}
                  </strong>
                </div>
              </div>

              {launchPreview && <pre className="command-preview">{launchPreview}</pre>}
            </div>

            <div className="panel">
              <div className="panel-heading">
                <div>
                  <p className="eyebrow">Download</p>
                  <h2>下载状态</h2>
                </div>
                <Download size={20} />
              </div>
              {downloadLoaderPicker}
              <div className="progress-block">
                <div className="progress-meter">
                  <span style={{ width: `${progressPercent}%` }} />
                </div>
                <strong>{progress ? `${progressPercent}%` : "空闲"}</strong>
                <p>{progress?.label ?? "选择一个版本后即可下载客户端、依赖库和资源文件"}</p>
              </div>
            </div>

            <div className="panel">
              <div className="panel-heading">
                <div>
                  <p className="eyebrow">Latest</p>
                  <h2>官方版本</h2>
                </div>
              </div>
              <button className="version-shortcut" onClick={() => setSelectedVersion(latestRelease)}>
                <span>正式版</span>
                <strong>{latestRelease}</strong>
              </button>
              <button className="version-shortcut" onClick={() => setSelectedVersion(latestSnapshot)}>
                <span>快照版</span>
                <strong>{latestSnapshot}</strong>
              </button>
            </div>
          </section>
        )}

        {activeTab === "versions" && (
          <section className="versions-layout">
            <div className="toolbar">
              <div className="search-box">
                <Search size={18} />
                <input
                  value={versionQuery}
                  onChange={(event) => setVersionQuery(event.target.value)}
                  placeholder="搜索版本号"
                />
              </div>
              <div className="segmented">
                {(["release", "snapshot", "all"] as VersionFilter[]).map((filter) => (
                  <button
                    key={filter}
                    className={versionFilter === filter ? "active" : ""}
                    onClick={() => setVersionFilter(filter)}
                  >
                    {filter === "release" ? "正式" : filter === "snapshot" ? "快照" : "全部"}
                  </button>
                ))}
              </div>
              <button className="secondary-action compact" onClick={chooseMinecraftRoot} disabled={busy === "choose-root"}>
                {busy === "choose-root" ? <Loader2 className="spin" size={18} /> : <FolderOpen size={18} />}
                <span>选择 .minecraft</span>
              </button>
            </div>

            <div className="version-list">
              <div className="version-section-title">本地版本</div>
              {visibleInstalled.length === 0 ? (
                <div className="empty-row">当前 .minecraft 未识别到本地版本</div>
              ) : (
                visibleInstalled.map((version) => (
                  <button
                    key={`installed-${version.id}`}
                    className={selectedVersion === version.id ? "version-row active" : "version-row"}
                    onClick={() => {
                      setSelectedVersion(version.id);
                      inspectVersion(version.id);
                    }}
                  >
                    <div>
                      <strong>{version.display_name || version.id}</strong>
                      <span>{version.inherits_from ? `继承 ${version.inherits_from}` : version.path}</span>
                    </div>
                    <span className={version.has_client ? "badge ok" : "badge"}>{version.loader}</span>
                  </button>
                ))
              )}
              <div className="version-section-title">官方下载</div>
              {visibleVersions.map((version) => {
                const installedVersion = installed.find((item) => item.id === version.id);
                return (
                  <button
                    key={version.id}
                    className={selectedVersion === version.id ? "version-row active" : "version-row"}
                    onClick={() => {
                      setSelectedVersion(version.id);
                      inspectVersion(version.id);
                    }}
                  >
                    <div>
                      <strong>{version.id}</strong>
                      <span>{new Date(versionReleaseTime(version)).toLocaleDateString("zh-CN")}</span>
                    </div>
                    <span className={installedVersion?.has_client ? "badge ok" : "badge"}>{versionType(version)}</span>
                  </button>
                );
              })}
            </div>

            <div className="panel version-detail">
              <div className="panel-heading">
                <div>
                  <p className="eyebrow">Metadata</p>
                  <h2>{summary?.id ?? selectedVersion}</h2>
                </div>
                <button className="icon-button" onClick={() => inspectVersion()} title="解析版本元数据">
                  {busy === "summary" ? <Loader2 className="spin" size={18} /> : <RefreshCw size={18} />}
                </button>
              </div>
              {summary ? (
                <div className="detail-grid">
                  <span>主类</span>
                  <strong>{summary.main_class}</strong>
                  <span>资源索引</span>
                  <strong>{summary.asset_index}</strong>
                  <span>Java</span>
                  <strong>{summary.java_major ? `${summary.java_component} / ${summary.java_major}` : "未声明"}</strong>
                  <span>依赖库</span>
                  <strong>{summary.libraries}</strong>
                  <span>资源文件</span>
                  <strong>{summary.assets ?? "未知"}</strong>
                  <span>客户端</span>
                  <strong>{formatBytes(summary.client_size)}</strong>
                </div>
              ) : (
                <p className="muted">选择版本后会显示 main class、Java 需求、资源索引和依赖数量。</p>
              )}
              {downloadLoaderPicker}
              <div className="action-row">
                <button className="primary-action" onClick={() => setActiveTab("play")} disabled={!selectedVersion}>
                  <Play size={18} />
                  <span>使用此版本</span>
                </button>
                <button className="secondary-action" onClick={downloadSelected} disabled={busy === "download"}>
                  <Download size={18} />
                  <span>下载 {selectedVersion}</span>
                </button>
                {selectedInstalledInfo && (
                  <button className="secondary-action danger" onClick={deleteSelectedVersion} disabled={busy === "delete-version"}>
                    {busy === "delete-version" ? <Loader2 className="spin" size={18} /> : <Trash2 size={18} />}
                    <span>删除版本</span>
                  </button>
                )}
              </div>
            </div>
          </section>
        )}

        {activeTab === "mods" && (
          <section className="mods-layout">
            <div className="toolbar">
              <div className="segmented">
                {(["mod", "shader", "resourcepack", "modpack"] as ModProjectType[]).map((type) => (
                  <button
                    key={type}
                    className={modProjectType === type ? "active" : ""}
                    onClick={() => setModProjectType(type)}
                  >
                    {modProjectLabel(type)}
                  </button>
                ))}
              </div>
              <div className="search-box">
                <Search size={18} />
                <input
                  value={modQuery}
                  onChange={(event) => setModQuery(event.target.value)}
                  placeholder={`搜索 Modrinth ${modProjectLabel(modProjectType)}`}
                />
              </div>
              <select
                className="mod-version-select"
                value={modGameVersion}
                onChange={(event) => setModGameVersion(event.target.value)}
                title="目标 Minecraft 版本"
              >
                <option value={selectedInstalledInfo?.inherits_from || selectedVersion || latestRelease}>
                  当前版本 {selectedInstalledInfo?.inherits_from || selectedVersion || latestRelease}
                </option>
                {manifest?.versions.slice(0, 160).map((version) => (
                  <option key={`mod-version-${version.id}`} value={version.id}>
                    {version.id}
                  </option>
                ))}
              </select>
              <select
                value={settings.loader}
                disabled={modProjectType === "shader" || modProjectType === "resourcepack"}
                onChange={(event) => setSettings({ ...settings, loader: event.target.value })}
              >
                <option value="fabric">Fabric</option>
                <option value="forge">Forge</option>
                <option value="quilt">Quilt</option>
                <option value="neoforge">NeoForge</option>
              </select>
              <button className="primary-action compact" onClick={searchMods} disabled={busy === "mods"}>
                {busy === "mods" ? <Loader2 className="spin" size={18} /> : <PackageSearch size={18} />}
                <span>搜索</span>
              </button>
            </div>

            <div className="mods-summary">
              <strong>{modTotal ? `${modTotal} 个${modProjectLabel(modProjectType)}结果` : "Modrinth 社区"}</strong>
              <span>
                目标版本 {targetModGameVersion}，
                {modProjectType === "mod" || modProjectType === "modpack" ? `加载器 ${settings.loader}，` : ""}
                安装到 {modProjectTarget(modProjectType)}
              </span>
              {lastInstall && <span>最近安装：{lastInstall.file_name}</span>}
            </div>

            <div className="mods-grid">
              {modResults.map((mod) => (
                <article className="mod-card" key={mod.project_id}>
                  <img src={mod.icon_url || ""} alt="" />
                  <div className="mod-copy">
                    <strong>{mod.title}</strong>
                    <p>{mod.description}</p>
                    <span>{compactNumber(mod.downloads)} 下载</span>
                  </div>
                  <button
                    className="icon-button"
                    onClick={() => openModVersionPicker(mod)}
                    title="选择版本并安装"
                    disabled={busy === `mod-${mod.project_id}`}
                  >
                    {busy === `mod-${mod.project_id}` ? <Loader2 className="spin" size={18} /> : <Download size={18} />}
                  </button>
                </article>
              ))}
            </div>
          </section>
        )}

        {activeTab === "account" && (
          <section className="page-grid account-grid">
            <div className="panel">
              <div className="panel-heading">
                <div>
                  <p className="eyebrow">Microsoft</p>
                  <h2>正版验证</h2>
                </div>
                {profile ? <CheckCircle2 className="ok-icon" size={22} /> : <LogIn size={22} />}
              </div>
              <p className="muted account-note">使用内置 Microsoft 登录应用获取登录码，不需要手动填写 Client ID。</p>

              <div className="action-row">
                <button className="primary-action" onClick={beginLogin} disabled={busy === "login"}>
                  {busy === "login" ? <Loader2 className="spin" size={18} /> : <LogIn size={18} />}
                  <span>{profile ? "切换账号" : "获取登录码"}</span>
                </button>
                {deviceCode && (
                  <button className="secondary-action" onClick={pollLogin}>
                    <RefreshCw size={18} />
                    <span>检查授权</span>
                  </button>
                )}
                {profile && profile.account_type !== "offline" && (
                  <>
                    <button className="secondary-action" onClick={() => refreshProfile()} disabled={busy === "refresh-login"}>
                      {busy === "refresh-login" ? <Loader2 className="spin" size={18} /> : <RefreshCw size={18} />}
                      <span>刷新登录</span>
                    </button>
                  </>
                )}
                {profile && (
                  <button className="secondary-action danger" onClick={logoutProfile} disabled={busy === "logout"}>
                    {busy === "logout" ? <Loader2 className="spin" size={18} /> : <LogOut size={18} />}
                    <span>退出账号</span>
                  </button>
                )}
              </div>

              {deviceCode && (
                <div className="device-code">
                  <span>用户码</span>
                  <strong>{deviceCode.user_code}</strong>
                  <a href={deviceCode.verification_uri} target="_blank" rel="noreferrer">
                    打开验证页 <ExternalLink size={15} />
                  </a>
                </div>
              )}
            </div>

            <div className="panel">
              <div className="panel-heading">
                <div>
                  <p className="eyebrow">Offline</p>
                  <h2>离线登录</h2>
                </div>
                <UserRound size={22} />
              </div>
              <label className="input-label">
                玩家名
                <input
                  value={offlineName}
                  onChange={(event) => setOfflineName(event.target.value)}
                  placeholder="3-16 位英文、数字或下划线"
                  maxLength={16}
                />
              </label>
              <div className="action-row">
                <button className="secondary-action" onClick={createOfflineAccount} disabled={busy === "offline-login"}>
                  {busy === "offline-login" ? <Loader2 className="spin" size={18} /> : <UserRound size={18} />}
                  <span>使用离线账号</span>
                </button>
              </div>
            </div>

            <div className="panel profile-panel">
              <div className="panel-heading">
                <div>
                  <p className="eyebrow">Profile</p>
                  <h2>{profile?.name ?? "未登录"}</h2>
                </div>
              </div>
              {profile ? (
                <>
                  <div
                    className={
                      profile.account_type === "offline" ? "license-card offline" : profile.owns_game ? "license-card ok" : "license-card warning"
                    }
                  >
                    {profile.account_type === "offline" ? <UserRound size={20} /> : profile.owns_game ? <ShieldCheck size={20} /> : <TriangleAlert size={20} />}
                    <strong>
                      {profile.account_type === "offline"
                        ? "离线账号，可启动单机与离线服务器"
                        : profile.owns_game
                          ? "已拥有 Minecraft Java Edition"
                          : "未检测到 Java Edition 授权"}
                    </strong>
                  </div>
                  <div className="detail-grid">
                    <span>UUID</span>
                    <strong>{profile.id}</strong>
                    <span>XUID</span>
                    <strong>{profile.account_type === "offline" ? "离线账号" : profile.xuid ?? "未返回"}</strong>
                    <span>令牌有效期</span>
                    <strong>{formatProfileExpiry(profile)}</strong>
                  </div>
                </>
              ) : (
                <p className="muted">登录成功后会在这里显示 Minecraft 档案和授权状态。</p>
              )}
            </div>
          </section>
        )}

        {activeTab === "settings" && (
          <section className="page-grid settings-grid">
            <div className="panel">
              <div className="panel-heading">
                <div>
                  <p className="eyebrow">Runtime</p>
                  <h2>运行参数</h2>
                </div>
              </div>
              <div className="input-label">
                Java
                <div className="java-mode-row">
                  <div className="segmented">
                    <button
                      className={settings.javaMode === "auto" ? "active" : ""}
                      onClick={() => setSettings({ ...settings, javaMode: "auto" })}
                    >
                      自动
                    </button>
                    <button
                      className={settings.javaMode === "manual" ? "active" : ""}
                      onClick={() => setSettings({ ...settings, javaMode: "manual" })}
                    >
                      手动
                    </button>
                  </div>
                  <button className="icon-button" onClick={() => refreshJavaInstallations()} title="搜索 Java" disabled={busy === "java-scan"}>
                    {busy === "java-scan" ? <Loader2 className="spin" size={18} /> : <RefreshCw size={18} />}
                  </button>
                  <button className="icon-button" onClick={chooseJavaExecutable} title="导入 Java" disabled={busy === "java-choose"}>
                    {busy === "java-choose" ? <Loader2 className="spin" size={18} /> : <FolderOpen size={18} />}
                  </button>
                </div>
                <select
                  value={settings.javaPath}
                  disabled={settings.javaMode !== "manual"}
                  onChange={(event) => setSettings({ ...settings, javaMode: "manual", javaPath: event.target.value })}
                >
                  <option value="">选择 Java</option>
                  {javaInstallations.map((java) => (
                    <option key={java.path} value={java.path}>
                      {java.display_name} · Java {java.major} · {java.source}
                    </option>
                  ))}
                </select>
                <input
                  value={settings.javaPath}
                  disabled={settings.javaMode !== "manual"}
                  onChange={(event) => setSettings({ ...settings, javaMode: "manual", javaPath: event.target.value })}
                  placeholder="java.exe 完整路径"
                />
                <div className="java-list">
                  {javaInstallations.slice(0, 6).map((java) => (
                    <button
                      key={`java-${java.path}`}
                      className={settings.javaMode === "manual" && settings.javaPath === java.path ? "java-row active" : "java-row"}
                      onClick={() => setSettings({ ...settings, javaMode: "manual", javaPath: java.path })}
                    >
                      <strong>{java.display_name}</strong>
                      <span>{java.path}</span>
                    </button>
                  ))}
                  {javaInstallations.length === 0 && <span className="muted">尚未搜索到 Java</span>}
                </div>
              </div>
              <div className="input-label">
                内存
                <div className="java-mode-row two-actions">
                  <div className="segmented">
                    <button
                      className={settings.memoryMode === "auto" ? "active" : ""}
                      onClick={() => setSettings({ ...settings, memoryMode: "auto" })}
                    >
                      自动
                    </button>
                    <button
                      className={settings.memoryMode === "manual" ? "active" : ""}
                      onClick={() => setSettings({ ...settings, memoryMode: "manual" })}
                    >
                      手动
                    </button>
                  </div>
                  <button
                    className="icon-button"
                    onClick={() => refreshMemoryRecommendation(selectedVersion)}
                    title="重新计算内存"
                    disabled={busy === "memory" || !selectedInstalled}
                  >
                    {busy === "memory" ? <Loader2 className="spin" size={18} /> : <RefreshCw size={18} />}
                  </button>
                </div>
                {settings.memoryMode === "auto" && (
                  <div className="memory-hint">
                    <strong>{memoryRecommendation ? `${memoryRecommendation.recommended_mb} MB` : "等待选择已安装版本"}</strong>
                    <span>
                      {memoryRecommendation
                        ? `${memoryRecommendation.reason} · 可用 ${memoryRecommendation.available_mb || "未知"} MB / 总计 ${memoryRecommendation.total_mb || "未知"} MB`
                        : "会按当前版本类型、隔离目录中的 Mod 数量和系统可用内存自动分配"}
                    </span>
                  </div>
                )}
                <div className="range-row">
                  <input
                    type="range"
                    min="1024"
                    max="16384"
                    step="512"
                    value={settings.maxMemoryMb}
                    disabled={settings.memoryMode !== "manual"}
                    onChange={(event) => setSettings({ ...settings, maxMemoryMb: Number(event.target.value) })}
                  />
                  <input
                    type="number"
                    min="1024"
                    max="32768"
                    step="512"
                    value={settings.maxMemoryMb}
                    disabled={settings.memoryMode !== "manual"}
                    onChange={(event) => setSettings({ ...settings, maxMemoryMb: Number(event.target.value) })}
                  />
                </div>
              </div>
            </div>

            <div className="panel paths-panel">
              <div className="panel-heading">
                <div>
                  <p className="eyebrow">Storage</p>
                  <h2>本地目录</h2>
                </div>
                <button className="icon-button" onClick={chooseMinecraftRoot} title="选择 .minecraft 文件夹" disabled={busy === "choose-root"}>
                  {busy === "choose-root" ? <Loader2 className="spin" size={18} /> : <FolderOpen size={18} />}
                </button>
              </div>
              <div className="path-list">
                <span>启动器</span>
                <code>{paths?.launcher_root ?? ""}</code>
                <span>Minecraft</span>
                <code>{paths?.minecraft_root ?? ""}</code>
                <span>版本</span>
                <code>{paths?.versions_root ?? ""}</code>
                <span>实例</span>
                <code>{paths?.instances_root ?? ""}</code>
              </div>
            </div>
          </section>
        )}
      </main>

      {modVersionDialog && (
        <div className="modal-backdrop" role="dialog" aria-modal="true" aria-label="选择资源版本">
          <div className="resource-modal">
            <div className="modal-heading">
              <div>
                <p className="eyebrow">Modrinth</p>
                <h2>{modVersionDialog.project.title}</h2>
              </div>
              <button className="icon-button" onClick={() => setModVersionDialog(null)} title="关闭">
                <X size={18} />
              </button>
            </div>

            <div className="resource-version-body">
              <label className="input-label">
                {modProjectLabel(modVersionDialog.projectType)}版本
                <select
                  value={modVersionDialog.selectedVersionId}
                  onChange={(event) =>
                    setModVersionDialog((current) =>
                      current ? { ...current, selectedVersionId: event.target.value } : current
                    )
                  }
                >
                  {modVersionDialog.versions.map((version) => (
                    <option key={version.id} value={version.id}>
                      {version.name || version.version_number}
                      {version.game_versions?.length ? ` · ${version.game_versions.slice(0, 3).join(", ")}` : ""}
                    </option>
                  ))}
                </select>
              </label>

              <div className="resource-version-info">
                <span>目标游戏版本 {targetModGameVersion}</span>
                <span>{modrinthVersionMeta(selectedModrinthVersion)}</span>
                <span>
                  默认下载到 <code>{modVersionDialog.defaultTarget}</code>
                </span>
              </div>
            </div>

            <div className="action-row modal-actions">
              <button
                className="primary-action"
                onClick={() => installSelectedModrinthVersion(true)}
                disabled={modVersionInstallBusy}
              >
                {modVersionInstallBusy ? <Loader2 className="spin" size={18} /> : <FolderOpen size={18} />}
                <span>选择文件夹并下载</span>
              </button>
              <button
                className="secondary-action"
                onClick={() => installSelectedModrinthVersion(false)}
                disabled={modVersionInstallBusy}
              >
                <Download size={18} />
                <span>下载到默认目录</span>
              </button>
            </div>
          </div>
        </div>
      )}

      {launchLogOpen && (
        <div className="modal-backdrop" role="dialog" aria-modal="true" aria-label="启动日志">
          <div className="launch-log-modal">
            <div className="modal-heading">
              <div>
                <p className="eyebrow">Launch Log</p>
                <h2>启动日志</h2>
              </div>
              <div className="top-actions">
                <button className="secondary-action compact" onClick={copyLaunchLog}>
                  <ClipboardCopy size={18} />
                  <span>复制日志</span>
                </button>
                <button className="icon-button" onClick={() => setLaunchLogOpen(false)} title="关闭日志">
                  <X size={18} />
                </button>
              </div>
            </div>
            <textarea className="launch-log-textarea" value={launchLog} readOnly spellCheck={false} />
          </div>
        </div>
      )}
    </div>
  );
}

export default App;
