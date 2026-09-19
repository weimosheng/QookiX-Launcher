import home from "./_fragments/home.zh";
import browse from "./_fragments/browse.zh";
import onboardingBar from "./_fragments/onboardingBar.zh";
import accountChip from "./_fragments/accountChip.zh";
import fileManager from "./_fragments/fileManager.zh";
import serverFileManager from "./_fragments/serverFileManager.zh";
import codeEditor from "./_fragments/codeEditor.zh";
import crashDialog from "./_fragments/crashDialog.zh";
import closeConfirm from "./_fragments/closeConfirm.zh";
import diagnostics from "./_fragments/diagnostics.zh";
import iconPicker from "./_fragments/iconPicker.zh";
import updaterCheck from "./_fragments/updaterCheck.zh";
import splash from "./_fragments/splash.zh";
import launchProgress from "./_fragments/launchProgress.zh";
import installDialog from "./_fragments/installDialog.zh";
import cloudSync from "./_fragments/cloudSync.zh";
import msLogin from "./_fragments/msLogin.zh";
import yggLogin from "./_fragments/yggLogin.zh";
import simplePagination from "./_fragments/simplePagination.zh";
import instanceSettings from "./_fragments/instanceSettings.zh";
import instanceContent from "./_fragments/instanceContent.zh";
import instanceSaves from "./_fragments/instanceSaves.zh";
import exportDialog from "./_fragments/exportDialog.zh";
import instances from "./_fragments/instances.zh";
import instanceDetail from "./_fragments/instanceDetail.zh";
import createInstance from "./_fragments/createInstance.zh";
import projectCard from "./_fragments/projectCard.zh";
import playtimeCard from "./_fragments/playtimeCard.zh";
import instanceCard from "./_fragments/instanceCard.zh";
import logViewer from "./_fragments/logViewer.zh";
import crashAnalyzer from "./_fragments/crashAnalyzer.zh";
import multiplayer from "./_fragments/multiplayer.zh";
import serverDetail from "./_fragments/serverDetail.zh";
import downloads from "./_fragments/downloads.zh";
import skinZh from "./_fragments/skin.zh";
import newsZh from "./_fragments/news.zh";
import toolboxZh from "./_fragments/toolbox.zh";
import seedMapZh from "./_fragments/seedMap.zh";
import settingsZh from "./_fragments/settings.zh";

export default {
  ...home,
  ...browse,
  multiplayer,
  serverDetail,
  downloads,
  common: {
    confirm: "确认",
    cancel: "取消",
    save: "保存",
    delete: "删除",
    edit: "编辑",
    close: "关闭",
    refresh: "刷新",
    loading: "加载中…",
    retry: "重试",
    ok: "确定",
  },
  nav: {
    home: "首页",
    browse: "内容",
    instances: "实例",
    multiplayer: "多人",
    skins: "皮肤",
    toolbox: "工具箱",
    settings: "设置",
    news: "新闻",
    downloads: "下载",
  },
  page: {
    instanceDetail: "实例详情",
    create: "创建实例",
    serverDetail: "服务器详情",
    seedMap: "种子地图",
    newInstance: "新建实例",
  },
  format: {
    justNow: "刚刚",
    minutesAgo: "{n} 分钟前",
    hoursAgo: "{n} 小时前",
    yesterday: "昨天",
    dayBeforeYesterday: "前天",
    daysAgo: "{n} 天前",
    lastMonth: "上个月",
    monthsAgo: "{n} 个月前",
    lastYear: "去年",
    yearsAgo: "{n} 年前",
    durationHours: "{n} 小时",
    durationMinutes: "{n} 分钟",
    durationSeconds: "{n} 秒",
    vanilla: "原版",
  },
  sidebar: {
    pinned: "固定",
    unpin: "取消固定",
    stopAll: "关闭所有实例",
    stopAllDone: "已关闭所有实例",
    downloading: "正在下载 {count} 项",
  },
  titlebar: {
    title: "QookiX Launcher",
    minimize: "最小化",
    maximize: "最大化",
    restore: "还原",
    close: "关闭",
    updateReady: "重启以更新",
    updateInstalling: "正在安装…",
    updateTooltip: "已下载 v{version}，点击安装并重启",
    updateTooltipGeneric: "已下载更新，点击安装并重启",
    newGroup: "新建分组",
    createServer: "创建服务器",
    clearFinished: "清除已完成",
  },
  settings: {
    ...settingsZh,
    language: {
      label: "语言",
      desc: "切换界面显示语言",
      zhCN: "简体中文",
      enUS: "English",
    },
    theme: {
      label: "主题",
      dark: "深色",
      light: "浅色",
    },
  },
  boot: {
    starting: "正在启动…",
    loadingSettings: "加载设置…",
    readingData: "读取实例与账号…",
    initTasks: "初始化任务系统…",
    almostReady: "即将就绪…",
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
  ...skinZh,
  ...newsZh,
  ...toolboxZh,
  ...seedMapZh,
};
