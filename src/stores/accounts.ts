import { defineStore } from "pinia";
import { api } from "../api";
import { useSettingsStore } from "./settings";
import type { Account } from "../types";

export const useAccountsStore = defineStore("accounts", {
  state: () => ({
    accounts: [] as Account[],
    msFlow: null as { userCode: string; verificationUri: string } | null,
    msPolling: false,
    msError: "",
    msSuccess: "",
    msFailed: "",
    /** Set by the sidebar chip / home button to open the account manager popover */
    showManager: false,
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
    async load() {
      this.accounts = await api.listAccounts();
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
      this.msFlow = { userCode: info.userCode, verificationUri: info.verificationUri };
      this.pollMs();
    },
    async pollMs() {
      if (this.msPolling) return;
      this.msPolling = true;
      try {
        for (let i = 0; i < 120; i++) {
          if (!this.msFlow) return;
          try {
            const acc = await api.loginMsPoll();
            this.msFlow = null;
            this.msError = "";
            await this.load();
            await this.select(acc.uuid);
            this.msSuccess = `${acc.username} 登录成功`;
            return;
          } catch (e: unknown) {
            if (!this.msFlow) return;
            const msg = String(e);
            if (msg.includes("pending")) {
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
        await this.load();
        await this.select(acc.uuid);
        this.msSuccess = `${acc.username} 登录成功`;
      } catch (e: unknown) {
        const msg = String(e);
        if (!msg.includes("pending") && this.msFlow) {
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
  },
});
