import { defineStore } from "pinia";
import { api } from "../api";
import { useSettingsStore } from "./settings";
import type { Account } from "../types";

export const useAccountsStore = defineStore("accounts", {
  state: () => ({
    accounts: [] as Account[],
    msFlow: null as { userCode: string; verificationUri: string; expiresIn: number } | null,
    msPolling: false,
    msError: "",
    msSuccess: "",
    msFailed: "",
    /** Set by the sidebar chip / home button to open the account manager popover */
    showManager: false,
    /** Bumped after skin apply to force avatar refresh */
    avatarVersion: 0,
    /** 最近一次成功拉取的时间戳，用于短 TTL 内跳过重复请求 */
    lastLoadedAt: 0,
  }),
  getters: {
    /** The currently playing account: settings.selected_account, else first */
    current: (s): Account | null => {
      const settings = useSettingsStore();
      const sel = settings.settings?.selected_account;
      if (sel) {
        const found = s.accounts.find((a) => a.uuid === sel);
        if (found) return found;
      }
      return s.accounts[0] ?? null;
    },
  },
  actions: {
    /** 后台静默刷新账号列表，失败保留旧数据 */
    async refresh() {
      try {
        this.accounts = await api.listAccounts();
        this.lastLoadedAt = Date.now();
      } catch {
        /* 后台刷新失败保留旧数据 */
      }
    },
    /**
     * 拉取账号列表。
     * - 已有数据时采用 stale-while-revalidate：立即返回旧数据，后台静默刷新替换。
     * - `force=true` 时始终前台拉取（用于登录/登出后强制刷新）。
     */
    async load(force = false) {
      const now = Date.now();
      if (!force && this.lastLoadedAt && now - this.lastLoadedAt < 3000) return;
      const hasData = this.accounts.length > 0;
      if (hasData && !force) {
        void this.refresh();
        return;
      }
      this.accounts = await api.listAccounts();
      this.lastLoadedAt = Date.now();
    },
    /** Select the current playing account (persisted globally). */
    async select(uuid: string) {
      const settings = useSettingsStore();
      await settings.patch({ selected_account: uuid });
    },
    async addOffline(username: string) {
      const acc = await api.loginOffline(username);
      await this.load();
      // auto-select the new account
      await this.select(acc.uuid);
      return acc;
    },
    async startMs() {
      this.msError = "";
      const info = await api.loginMsStart();
      this.msFlow = { userCode: info.userCode, verificationUri: info.verificationUri, expiresIn: info.expiresIn };
      this.pollMs();
    },
    async pollMs() {
      if (this.msPolling) return;
      this.msPolling = true;
      try {
        const maxAttempts = this.msFlow ? Math.ceil(this.msFlow.expiresIn / 5) + 12 : 120;
        for (let i = 0; i < maxAttempts; i++) {
          if (!this.msFlow) return;
          try {
            const acc = await api.loginMsPoll();
            this.msFlow = null;
            this.msError = "";
            await this.load(true);
            await this.select(acc.uuid);
            this.msSuccess = `${acc.username} 登录成功`;
            return;
          } catch (e: unknown) {
            if (!this.msFlow) return;
            const msg = String(e);
            if (msg === "__auth_pending__") {
              await new Promise((r) => setTimeout(r, 5000));
              continue;
            }
            this.msError = msg.replace(/^Error:\s*/, "");
            return;
          }
        }
        this.msError = "等待超时，请重新尝试";
      } finally {
        this.msPolling = false;
      }
    },
    /** Manually trigger an immediate poll (skips the 5s wait). */
    async manualCheck() {
      if (!this.msFlow || this.msError) return;
      try {
        const acc = await api.loginMsPoll();
        this.msFlow = null;
        this.msError = "";
        this.msPolling = false;
        await this.load(true);
        await this.select(acc.uuid);
        this.msSuccess = `${acc.username} 登录成功`;
      } catch (e: unknown) {
        const msg = String(e);
        if (msg !== "__auth_pending__" && this.msFlow) {
          this.msError = msg.replace(/^Error:\s*/, "");
          this.msPolling = false;
        }
      }
    },
    async remove(uuid: string) {
      await api.logoutAccount(uuid);
      const settings = useSettingsStore();
      if (settings.settings?.selected_account === uuid) {
        await settings.patch({ selected_account: null });
      }
      await this.load();
    },
    bumpAvatar() {
      this.avatarVersion++;
    },
  },
});
