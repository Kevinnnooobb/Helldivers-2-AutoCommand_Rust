// UI 共享辅助：分类短标签

/// 分类短标签（终端风格）
pub fn cat_short(cat: &str) -> &'static str {
    match cat {
        "Mission Stratagems" => "Mission",
        "Orbital Strikes" => "Orbital",
        "Eagle Strikes" => "Eagle",
        "Support Weapons" => "Support",
        "Sentries" => "Sentries",
        "Emplacements" => "Emplace",
        "Backpacks" => "Backpacks",
        "Vehicles" => "Vehicles",
        "NEW (Wiki)" => "New",
        _ => "?",
    }
}
