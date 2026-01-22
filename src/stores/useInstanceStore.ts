/**
 * Instance Store - 实例状态管理
 * 使用 Zustand 管理实例列表和状态
 */

import { create } from 'zustand';
import { Instance } from '../types/instance';
import * as instanceService from '../services/instanceService';

interface InstanceState {
    instances: Instance[];
    loading: boolean;
    error: string | null;

    // Actions
    fetchInstances: () => Promise<void>;
    createInstance: (name: string, userDataDir: string, extraArgs?: string[]) => Promise<Instance>;
    deleteInstance: (instanceId: string) => Promise<void>;
    updateInstance: (instance: Instance) => Promise<void>;
    bindAccountToInstance: (accountId: string, instanceId: string) => Promise<void>;
    unbindAccountFromInstance: (accountId: string, instanceId: string) => Promise<void>;
    startInstance: (instanceId: string) => Promise<void>;
    stopInstance: (instanceId: string) => Promise<void>;
    getInstanceStatus: (instanceId: string) => Promise<boolean>;
    ensureDefaultInstance: () => Promise<Instance>;
    migrateAccountsToDefaultInstance: () => Promise<void>;
    getInstancesForAccount: (accountId: string) => Promise<Instance[]>;
}

export const useInstanceStore = create<InstanceState>((set, get) => ({
    instances: [],
    loading: false,
    error: null,

    fetchInstances: async () => {
        set({ loading: true, error: null });
        try {
            const instances = await instanceService.listInstances();
            set({ instances, loading: false });
        } catch (error) {
            set({ error: String(error), loading: false });
        }
    },

    createInstance: async (name: string, userDataDir: string, extraArgs?: string[]) => {
        const instance = await instanceService.createInstance(name, userDataDir, extraArgs);
        await get().fetchInstances();
        return instance;
    },

    deleteInstance: async (instanceId: string) => {
        await instanceService.deleteInstance(instanceId);
        await get().fetchInstances();
    },

    updateInstance: async (instance: Instance) => {
        await instanceService.updateInstance(instance);
        await get().fetchInstances();
    },

    bindAccountToInstance: async (accountId: string, instanceId: string) => {
        await instanceService.bindAccountToInstance(accountId, instanceId);
        await get().fetchInstances();
    },

    unbindAccountFromInstance: async (accountId: string, instanceId: string) => {
        await instanceService.unbindAccountFromInstance(accountId, instanceId);
        await get().fetchInstances();
    },

    startInstance: async (instanceId: string) => {
        await instanceService.startInstance(instanceId);
    },

    stopInstance: async (instanceId: string) => {
        await instanceService.stopInstance(instanceId);
    },

    getInstanceStatus: async (instanceId: string) => {
        return await instanceService.getInstanceStatus(instanceId);
    },

    ensureDefaultInstance: async () => {
        const instance = await instanceService.ensureDefaultInstance();
        await get().fetchInstances();
        return instance;
    },

    migrateAccountsToDefaultInstance: async () => {
        await instanceService.migrateAccountsToDefaultInstance();
        await get().fetchInstances();
    },

    getInstancesForAccount: async (accountId: string) => {
        return await instanceService.getInstancesForAccount(accountId);
    },
}));
