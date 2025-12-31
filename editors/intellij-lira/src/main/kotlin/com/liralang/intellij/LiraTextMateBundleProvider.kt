package com.liralang.intellij

import org.jetbrains.plugins.textmate.api.TextMateBundleProvider
import org.jetbrains.plugins.textmate.api.TextMateBundle

class LiraTextMateBundleProvider : TextMateBundleProvider {
    override fun getBundles(): List<TextMateBundle> {
        val bundlePath = this::class.java.classLoader.getResource("textmate/lira")?.path
            ?: return emptyList()

        return listOf(TextMateBundle("Lira", bundlePath))
    }
}
