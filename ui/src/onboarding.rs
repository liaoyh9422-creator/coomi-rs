use anyhow::Context;
use anyhow::Result;
use coomi_services::ProviderDocument;
use coomi_services::ProviderRegistry;
use coomi_services::ProviderSettings;
use std::collections::BTreeMap;
use std::io;
use std::io::IsTerminal;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;

pub fn load_registry_or_prompt(home: &Path) -> Result<ProviderRegistry> {
    let path = providers_path(home);
    match ProviderRegistry::load(&path) {
        Ok(registry) => Ok(registry),
        Err(error) => {
            if !can_prompt() || !can_create_first_provider(&path)? {
                return Err(error).with_context(|| {
                    format!("无法从 {} 加载模型；请先配置至少一个供应商", path.display())
                });
            }
            create_first_provider(&path)?;
            ProviderRegistry::load(&path)
                .with_context(|| format!("完成设置后仍无法从 {} 加载模型", path.display()))
        }
    }
}

fn providers_path(home: &Path) -> PathBuf {
    home.join("config").join("providers.json")
}

fn can_prompt() -> bool {
    io::stdin().is_terminal() && io::stderr().is_terminal()
}

fn can_create_first_provider(path: &Path) -> Result<bool> {
    if !path.exists() {
        return Ok(true);
    }
    let document = ProviderDocument::load(path)?;
    Ok(document.providers.is_empty())
}

fn create_first_provider(path: &Path) -> Result<()> {
    eprintln!("当前尚未配置可用的 Coomi 供应商。");
    eprintln!("现在创建第一个供应商。API Key 将以明文保存在 JSON 配置文件中。");

    let raw_id = prompt_default("供应商 ID", "default")?;
    let id = sanitize_id(&raw_id);
    let display = prompt_default("显示名称", &id)?;
    let protocol = prompt_default("协议类型", "openai_compatible")?;
    let base_url = prompt_default("服务地址（Base URL）", "https://api.openai.com/v1")?;
    let model = prompt_required("模型")?;
    let fast_model = prompt_optional("快速模型（可选）")?;
    let api_key = prompt_optional("API 密钥（可选，输入内容会明文显示）")?;

    let provider = ProviderSettings {
        provider_type: protocol.clone(),
        tool_protocol: Some(protocol),
        display,
        api_key,
        base_url,
        model,
        fast_model: non_empty(fast_model),
        ..ProviderSettings::default()
    };
    let document = ProviderDocument {
        active: id.clone(),
        providers: BTreeMap::from([(id, provider)]),
        extra: BTreeMap::new(),
    };
    document.save(path)?;
    eprintln!("供应商配置已保存到 {}", path.display());
    Ok(())
}

fn prompt_required(label: &str) -> Result<String> {
    loop {
        let value = prompt(label, None)?;
        if !value.trim().is_empty() {
            return Ok(value);
        }
        eprintln!("{}不能为空。", label);
    }
}

fn prompt_default(label: &str, default: &str) -> Result<String> {
    let value = prompt(label, Some(default))?;
    if value.trim().is_empty() {
        Ok(default.to_owned())
    } else {
        Ok(value)
    }
}

fn prompt_optional(label: &str) -> Result<String> {
    prompt(label, None)
}

fn prompt(label: &str, default: Option<&str>) -> Result<String> {
    match default {
        Some(default) => eprint!("{label} [{default}]: "),
        None => eprint!("{label}: "),
    }
    io::stderr().flush()?;
    let mut input = String::new();
    let bytes = io::stdin().read_line(&mut input)?;
    anyhow::ensure!(bytes > 0, "供应商配置已取消");
    Ok(input.trim().to_owned())
}

fn non_empty(value: String) -> Option<String> {
    (!value.trim().is_empty()).then_some(value)
}

fn sanitize_id(value: &str) -> String {
    let mut id = String::new();
    for character in value.trim().chars() {
        if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
            id.push(character.to_ascii_lowercase());
        } else if character.is_whitespace() && !id.ends_with('-') {
            id.push('-');
        }
    }
    let id = id.trim_matches('-').to_owned();
    if id.is_empty() { "default".into() } else { id }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_ids_are_sanitized() {
        assert_eq!(sanitize_id("My Provider"), "my-provider");
        assert_eq!(sanitize_id("***"), "default");
    }
}
