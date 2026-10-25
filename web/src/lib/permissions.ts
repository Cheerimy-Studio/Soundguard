import type { UserRole } from "../types";
export const roleLabels: Record<UserRole, string> = {
  super_admin: "超级管理员", update_admin: "运营管理员", device_admin: "设备管理员",
};
export function canOperate(role?: UserRole) { return role === "super_admin" || role === "update_admin"; }
export const rolePermissions = [
  { label: "监测、对讲和证据", values: ["全部设备", "全部设备", "分配的设备"] },
  { label: "设备接入和令牌", values: ["可管理", "可管理", "不可管理"] },
  { label: "全局策略和计划任务", values: ["可修改", "可修改", "仅查看"] },
  { label: "账号和权限分配", values: ["可修改", "仅查看", "不可查看"] },
  { label: "密钥、固定密码、留存和升级", values: ["可修改", "不可修改", "不可修改"] },
];
