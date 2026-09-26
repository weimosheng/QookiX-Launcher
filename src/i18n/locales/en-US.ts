import home from "./_fragments/home.en";
import browse from "./_fragments/browse.en";
import onboardingBar from "./_fragments/onboardingBar.en";
import accountChip from "./_fragments/accountChip.en";
import fileManager from "./_fragments/fileManager.en";
import serverFileManager from "./_fragments/serverFileManager.en";
import codeEditor from "./_fragments/codeEditor.en";
import crashDialog from "./_fragments/crashDialog.en";
import closeConfirm from "./_fragments/closeConfirm.en";
import diagnostics from "./_fragments/diagnostics.en";
import iconPicker from "./_fragments/iconPicker.en";
import updaterCheck from "./_fragments/updaterCheck.en";
import splash from "./_fragments/splash.en";
import launchProgress from "./_fragments/launchProgress.en";
import installDialog from "./_fragments/installDialog.en";
import cloudSync from "./_fragments/cloudSync.en";
import msLogin from "./_fragments/msLogin.en";
import yggLogin from "./_fragments/yggLogin.en";
import simplePagination from "./_fragments/simplePagination.en";
import instanceSettings from "./_fragments/instanceSettings.en";
import instanceContent from "./_fragments/instanceContent.en";
import instanceSaves from "./_fragments/instanceSaves.en";
import exportDialog from "./_fragments/exportDialog.en";
import instances from "./_fragments/instances.en";
import instanceDetail from "./_fragments/instanceDetail.en";
import createInstance from "./_fragments/createInstance.en";
import projectCard from "./_fragments/projectCard.en";
import playtimeCard from "./_fragments/playtimeCard.en";
import instanceCard from "./_fragments/instanceCard.en";
import logViewer from "./_fragments/logViewer.en";
import crashAnalyzer from "./_fragments/crashAnalyzer.en";
import multiplayer from "./_fragments/multiplayer.en";
import serverDetail from "./_fragments/serverDetail.en";
import downloads from "./_fragments/downloads.en";
import skinEn from "./_fragments/skin.en";
import newsEn from "./_fragments/news.en";
import toolboxEn from "./_fragments/toolbox.en";
import seedMapEn from "./_fragments/seedMap.en";
import settingsEn from "./_fragments/settings.en";

export default {
  ...home,
  ...browse,
  multiplayer,
  serverDetail,
  downloads,
  common: {
    confirm: "Confirm",
    cancel: "Cancel",
    save: "Save",
    delete: "Delete",
    edit: "Edit",
    close: "Close",
    refresh: "Refresh",
    loading: "Loading…",
    retry: "Retry",
    ok: "OK",
  },
  nav: {
    home: "Home",
    browse: "Browse",
    instances: "Instances",
    multiplayer: "Multiplayer",
    skins: "Skins",
    toolbox: "Toolbox",
    settings: "Settings",
    news: "News",
    downloads: "Downloads",
  },
  page: {
    instanceDetail: "Instance details",
    create: "Create instance",
    serverDetail: "Server details",
    seedMap: "Seed map",
    schematicPreview: "Schematic Workshop",
    newInstance: "New instance",
  },
  format: {
    justNow: "just now",
    minutesAgo: "{n} min ago",
    hoursAgo: "{n} h ago",
    yesterday: "yesterday",
    dayBeforeYesterday: "2 days ago",
    daysAgo: "{n} d ago",
    lastMonth: "last month",
    monthsAgo: "{n} months ago",
    lastYear: "last year",
    yearsAgo: "{n} years ago",
    durationHours: "{n} h",
    durationMinutes: "{n} min",
    durationSeconds: "{n}s",
    vanilla: "Vanilla",
  },
  sidebar: {
    pinned: "Pinned",
    unpin: "Unpin",
    stopAll: "Stop all instances",
    stopAllDone: "All instances stopped",
    downloading: "Downloading {count} item(s)",
  },
  titlebar: {
    title: "QookiX Launcher",
    minimize: "Minimize",
    maximize: "Maximize",
    restore: "Restore",
    close: "Close",
    updateReady: "Restart to update",
    updateInstalling: "Installing…",
    updateTooltip: "v{version} downloaded, click to install and restart",
    updateTooltipGeneric: "Update downloaded, click to install and restart",
    newGroup: "New group",
    createServer: "Create server",
    clearFinished: "Clear finished",
  },
  settings: {
    ...settingsEn,
    language: {
      label: "Language",
      desc: "Switch the interface display language",
      zhCN: "简体中文",
      enUS: "English",
    },
    theme: {
      label: "Theme",
      dark: "Dark",
      light: "Light",
    },
  },
  boot: {
    starting: "Starting…",
    loadingSettings: "Loading settings…",
    readingData: "Reading instances and accounts…",
    initTasks: "Initializing task system…",
    almostReady: "Almost ready…",
  },
  onboardingBar,
  accountChip,
  fileManager,
  serverFileManager,
  codeEditor,
  crashDialog,
  closeConfirm,
  diagnostics,
  iconPicker,
  updaterCheck,
  splash,
  launchProgress,
  installDialog,
  cloudSync,
  msLogin,
  yggLogin,
  simplePagination,
  instanceSettings,
  instanceContent,
  instanceSaves,
  exportDialog,
  instances,
  instanceDetail,
  createInstance,
  projectCard,
  playtimeCard,
  instanceCard,
  logViewer,
  crashAnalyzer,
  ...skinEn,
  ...newsEn,
  ...toolboxEn,
  ...seedMapEn,
};
