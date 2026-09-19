export default {
  browse: {
    type: {
      mod: "Mods",
      modpack: "Modpacks",
      resourcepack: "Resource Packs",
      shader: "Shaders",
      datapack: "Datapacks",
    },
    loader: {
      all: "All loaders",
    },
    provider: {
      all: "All sources",
    },
    sort: {
      downloads: "Downloads",
      relevance: "Relevance",
      follows: "Follows",
      newest: "Newest",
      updated: "Recently updated",
    },
    pageSize: {
      perPage: "{count} / page",
    },
    instance: {
      none: "No instance",
      searchPlaceholder: "Search instance / version / loader",
    },
    version: {
      all: "All versions",
    },
    category: {
      all: "All categories",
    },
    search: {
      placeholder: "Search… (e.g. sodium / iris / a modpack)",
      searching: "Searching…",
      noResult: "No results found",
    },
    view: {
      grid: "Grid",
      list: "List",
      compact: "Compact",
    },
    filter: {
      button: "Filter",
      title: "Filter",
      gameVersion: "Game version",
      loader: "Loader",
      category: "Category",
      versionTag: "Version {value}",
      loaderTag: "Loader {value}",
      categoryTag: "Category {value}",
      remove: "Remove",
      clearAll: "Clear all",
      reset: "Reset",
      done: "Done",
      hint: "Under All sources, categories filter by Modrinth; CurseForge results are unaffected",
    },
    translate: {
      toggle: "Translate",
      done: "Done translating",
      modeHint:
        "Translate mode: click a card to translate, click the button again to exit",
      busy: "Translation service is busy, please try again later",
      noTranslation: "No translation available for this content",
      openBrowserFailed: "Failed to open browser: {error}",
    },
    cf: {
      needApiKeyPre: "CurseForge requires an API Key. Please visit",
      needApiKeyMid: "to apply for free, and fill it in",
      needApiKeyPost: ".",
      loadFailedPre: "CurseForge failed to load: {error}. Please go to",
      loadFailedPost: "to check the API Key.",
      settings: "Settings",
    },
    pager: {
      total: "{count} results",
    },
  },
};
