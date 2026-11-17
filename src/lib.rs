//! nov-nas-back：群晖 NAS 离线增量备份工具。
//!
//! 公共模块：
//! - [`cli`]：命令行参数
//! - [`config`]：配置加载与校验
//! - [`backup`]：备份引擎（遍历、比对、拷贝）
//! - [`throttle`]：全局限速器
//! - [`error`]：领域错误

pub mod backup;
pub mod cli;
pub mod config;
pub mod error;
pub mod throttle;
