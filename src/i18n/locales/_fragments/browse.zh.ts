export default {
  browse: {
    type: {
      mod: "模组",
      modpack: "整合包",
      resourcepack: "资源包",
      shader: "光影",
      datapack: "数据包",
    },
    loader: {
      all: "全部加载器",
    },
    provider: {
      all: "全部来源",
    },
    sort: {
      downloads: "下载量",
      relevance: "相关度",
      follows: "收藏数",
      newest: "最新发布",
      updated: "最近更新",
    },
    pageSize: {
      perPage: "{count} 条 / 页",
    },
    instance: {
      none: "不关联实例",
      searchPlaceholder: "搜索实例名 / 版本 / 加载器",
    },
    version: {
      all: "全部版本",
    },
    category: {
      all: "全部分类",
    },
    search: {
      placeholder: "搜索内容…（如 sodium / iris / 某整合包）",
      searching: "搜索中…",
      noResult: "没有找到相关内容",
    },
    view: {
      grid: "网格",
      list: "列表",
      compact: "紧凑列表",
    },
    filter: {
      button: "筛选",
      title: "筛选",
      gameVersion: "游戏版本",
      loader: "加载器",
      category: "类别",
      versionTag: "版本 {value}",
      loaderTag: "加载器 {value}",
      categoryTag: "分类 {value}",
      remove: "移除",
      clearAll: "清除全部",
      reset: "重置",
      done: "完成",
      hint: "全部来源下分类按 Modrinth 筛选，CurseForge 结果不受分类影响",
    },
    translate: {
      toggle: "翻译",
      done: "完成翻译",
      modeHint: "翻译模式：点击要翻译的卡片，完成后再点一次按钮退出",
      busy: "翻译服务繁忙，请稍后再试",
      noTranslation: "该内容暂时没有翻译",
      openBrowserFailed: "打开浏览器失败：{error}",
    },
    cf: {
      needApiKeyPre: "CurseForge 需要 API Key。请前往",
      needApiKeyMid: "免费申请，并在",
      needApiKeyPost: "中填写。",
      loadFailedPre: "CurseForge 来源加载失败：{error}。请前往",
      loadFailedPost: "检查 API Key。",
      settings: "设置",
    },
    pager: {
      total: "共 {count} 条",
    },
  },
};
