# Lira Lira UI Format Specification

## Document Information

| Property | Value |
|----------|-------|
| **Document ID** | 20-liui-format |
| **Version** | 1.0.0-draft |
| **Status** | Draft Specification |
| **File Extension** | `.liui` |

---

## Table of Contents

1. [Overview](#1-overview)
2. [File Structure](#2-file-structure)
3. [Component Syntax](#3-component-syntax)
4. [Properties](#4-properties)
5. [Data Binding](#5-data-binding)
6. [Event Handling](#6-event-handling)
7. [Conditional Rendering](#7-conditional-rendering)
8. [List Rendering](#8-list-rendering)
9. [Component Definition](#9-component-definition)
10. [Slots and Children](#10-slots-and-children)
11. [Styling](#11-styling)
12. [Animations](#12-animations)
13. [Integration with Lira](#13-integration-with-lira)

---

## 1. Overview

### 1.1 Purpose

Lira UI (Lira User Interface) is a declarative GUI format for building applications. It separates UI structure from logic while enabling seamless integration with Lira code.

### 1.2 Design Goals

1. **Declarative**: Describe what the UI should look like, not how to build it
2. **Reactive**: Automatic UI updates when data changes
3. **Composable**: Build complex UIs from reusable components
4. **Type-Safe**: Full type checking with Lira integration
5. **Familiar**: JSON-like syntax, similar to QML/JSX

### 1.3 File Organization

```
my_app/
├── src/
│   ├── main.li              # Application entry point
│   ├── state.li             # Application state
│   └── components/
│       └── custom_button.li # Custom component logic
├── ui/
│   ├── main.liui            # Main window UI
│   └── components/
│       └── custom_button.liui  # Custom component UI
└── li.toml
```

### 1.4 Example

```liui
// ui/counter.liui
import state from "../src/counter_state.li"

Window {
    title: "Counter"
    width: 300
    height: 200

    VBox {
        spacing: 16
        padding: 24
        align: "center"

        Label {
            text: ${"Count: " + state.count}
            style: {
                fontSize: 24
                fontWeight: "bold"
            }
        }

        HBox {
            spacing: 8

            Button {
                text: "-"
                onClick: () => state.count -= 1
            }

            Button {
                text: "+"
                primary: true
                onClick: () => state.count += 1
            }
        }
    }
}
```

---

## 2. File Structure

### 2.1 File Sections

A Lira UI file consists of:

```liui
// 1. Imports (optional)
import state from "./state.li"
import { CustomButton } from "./components/button.liui"

// 2. Component definitions (optional)
component MyButton {
    // ...
}

// 3. Root element (required)
Window {
    // ...
}
```

### 2.2 Import Statements

#### Import Lira Module

```liui
// Import entire module
import myModule from "./module.li"

// Import specific items
import { counter, increment } from "./state.li"

// Import with alias
import counter as myCounter from "./state.li"
```

#### Import Lira UI Components

```liui
// Import component
import { CustomButton } from "./components/button.liui"

// Import multiple components
import { Button, TextField, Card } from "./ui_kit.liui"

// Import with alias
import { Button as PrimaryButton } from "./buttons.liui"
```

#### Import Built-in Components

```liui
// Built-in widgets are available without import
// But can be explicitly imported:
import { VBox, HBox, Label } from "liui:widgets"
```

### 2.3 Comments

```liui
// Single-line comment

/*
   Multi-line
   comment
*/

/// Documentation comment for component
component MyWidget {
    // ...
}
```

---

## 3. Component Syntax

### 3.1 Basic Element

```liui
ElementName {
    property1: value1
    property2: value2

    // Child elements
    ChildElement { }
}
```

### 3.2 Property Types

```liui
Label {
    // String literal
    text: "Hello, World!"

    // Number literal
    fontSize: 16

    // Boolean literal
    visible: true

    // Object literal
    style: {
        color: "#333"
        margin: 8
    }

    // Array literal
    items: ["One", "Two", "Three"]

    // Expression
    width: ${parent.width / 2}

    // Binding expression
    text: ${state.message}

    // Two-way binding
    value: <=> state.inputText

    // Function/callback
    onClick: () => doSomething()
}
```

### 3.3 Children

```liui
VBox {
    // Explicit children property
    children: [
        Label { text: "First" }
        Label { text: "Second" }
    ]
}

// Shorthand: direct nesting
VBox {
    Label { text: "First" }
    Label { text: "Second" }
}
```

### 3.4 Text Content

```liui
// Text-only elements can use text property
Label { text: "Hello" }

// Or content property for multi-line
Label {
    content: """
        This is a longer
        multi-line text content.
    """
}
```

### 3.5 Spread Attributes

```liui
// Spread object properties
Button {
    ...buttonProps
    text: "Click Me"  // Can override spread properties
}
```

---

## 4. Properties

### 4.1 Static Properties

Values that don't change:

```liui
Window {
    title: "My App"           // String
    width: 800                // Integer
    height: 600               // Integer
    resizable: true           // Boolean
    opacity: 0.95             // Float
}
```

### 4.2 Object Properties

Nested property objects:

```liui
Button {
    style: {
        backgroundColor: "#007AFF"
        borderRadius: 8
        padding: {
            horizontal: 16
            vertical: 8
        }
    }
}
```

### 4.3 Array Properties

```liui
Select {
    options: [
        { value: "a", label: "Option A" }
        { value: "b", label: "Option B" }
        { value: "c", label: "Option C" }
    ]
}

// Simple array
TabView {
    tabs: ["Home", "Settings", "About"]
}
```

### 4.4 Reference Properties

Reference other elements by ID:

```liui
VBox {
    TextField {
        id: "nameInput"
        placeholder: "Enter name"
    }

    Label {
        // Reference the text field's value
        text: ${"Hello, " + nameInput.value}
    }
}
```

### 4.5 Property Shorthand

```liui
// When property name matches variable name
let buttonText = "Submit"
let buttonEnabled = true

Button {
    // Shorthand
    :text: buttonText
    :enabled: buttonEnabled

    // Equivalent to:
    // text: buttonText
    // enabled: buttonEnabled
}
```

---

## 5. Data Binding

### 5.1 One-Way Binding

UI updates when data changes:

```liui
// Expression binding with ${}
Label {
    text: ${state.userName}
}

// Complex expressions
Label {
    text: ${"Hello, " + state.userName + "!"}
    visible: ${state.items.length > 0}
    opacity: ${state.isActive ? 1.0 : 0.5}
}

// Computed values
ProgressBar {
    value: ${state.completed / state.total * 100}
}
```

### 5.2 Two-Way Binding

UI and data stay synchronized:

```liui
// Two-way binding with <=>
TextField {
    value: <=> state.inputText
}

// Equivalent to:
TextField {
    value: ${state.inputText}
    onInput: (v) => state.inputText = v
}

// Works with nested properties
Slider {
    value: <=> state.settings.volume
}

// With transform
TextField {
    value: <=> state.amount | number
}
```

### 5.3 Binding Modifiers

```liui
// Debounced binding (wait before updating)
TextField {
    value: <=> state.searchQuery | debounce(300)
}

// Throttled binding
Slider {
    value: <=> state.position | throttle(16)
}

// Lazy binding (update on blur/enter)
TextField {
    value: <=> state.email | lazy
}

// Format transform
Label {
    text: ${state.price | currency("USD")}
}
```

### 5.4 Reactive Expressions

```liui
// Expressions are automatically reactive
Label {
    // Updates when either state.firstName or state.lastName changes
    text: ${state.firstName + " " + state.lastName}
}

// Method calls are reactive if they depend on reactive state
Label {
    text: ${formatDate(state.createdAt)}
}

// Ternary expressions
Button {
    text: ${state.isEditing ? "Save" : "Edit"}
    style: {
        backgroundColor: ${state.isValid ? "#4CAF50" : "#F44336"}
    }
}
```

---

## 6. Event Handling

### 6.1 Event Handlers

```liui
Button {
    text: "Click Me"

    // Inline handler
    onClick: () => state.count += 1

    // Reference function
    onClick: handleClick

    // Handler with event parameter
    onClick: (event) => {
        console.log("Clicked at", event.x, event.y)
        doSomething()
    }
}
```

### 6.2 Common Events

```liui
Button {
    onClick: () => {}        // Click/tap
    onDoubleClick: () => {}  // Double click
    onLongPress: () => {}    // Long press/hold
}

TextField {
    onInput: (value) => {}   // Input changed
    onFocus: () => {}        // Gained focus
    onBlur: () => {}         // Lost focus
    onSubmit: () => {}       // Enter pressed
}

Slider {
    onChange: (value) => {}  // Value changed
    onDragStart: () => {}    // Started dragging
    onDragEnd: () => {}      // Stopped dragging
}

Window {
    onClose: () => {}        // Window closing
    onResize: (w, h) => {}   // Window resized
    onKeyDown: (key) => {}   // Key pressed
}
```

### 6.3 Event Modifiers

```liui
// Stop propagation
Button {
    onClick.stop: () => doSomething()
}

// Prevent default
Link {
    onClick.prevent: () => navigate("/about")
}

// Capture phase
Panel {
    onClick.capture: () => logClick()
}

// Self only (only when event.target == this element)
Container {
    onClick.self: () => closePanel()
}

// Once (remove handler after first trigger)
Button {
    onClick.once: () => initialize()
}

// Passive (for scroll performance)
ScrollView {
    onScroll.passive: () => updatePosition()
}

// Combine modifiers
Link {
    onClick.stop.prevent: () => handleClick()
}
```

### 6.4 Key Events

```liui
TextField {
    // Specific key handlers
    onKeyDown.enter: () => submit()
    onKeyDown.escape: () => cancel()
    onKeyDown.tab: () => nextField()

    // Modifiers
    onKeyDown.ctrl.s: () => save()
    onKeyDown.cmd.z: () => undo()
    onKeyDown.shift.tab: () => prevField()
}
```

---

## 7. Conditional Rendering

### 7.1 If/Else

```liui
VBox {
    // Simple condition
    @if (state.isLoggedIn) {
        Label { text: "Welcome back!" }
    }

    // If-else
    @if (state.isLoading) {
        Spinner { }
    } @else {
        Content { }
    }

    // If-else-if chain
    @if (state.status == "loading") {
        Spinner { }
    } @else if (state.status == "error") {
        ErrorMessage { text: ${state.error} }
    } @else if (state.status == "empty") {
        EmptyState { }
    } @else {
        DataList { items: ${state.data} }
    }
}
```

### 7.2 When Expression

Pattern-matching style conditional:

```liui
@when (state.userRole) {
    "admin" => AdminPanel { }
    "editor" => EditorPanel { }
    "viewer" => ViewerPanel { }
    _ => GuestPanel { }
}

// With guards
@when (state.count) {
    0 => Label { text: "None" }
    1 => Label { text: "One item" }
    n if n < 10 => Label { text: ${n + " items"} }
    _ => Label { text: "Many items" }
}
```

### 7.3 Show/Hide vs Render

```liui
// @if removes element from tree (doesn't render)
@if (state.showPanel) {
    ExpensiveComponent { }
}

// visible property hides but keeps in tree
Panel {
    visible: ${state.showPanel}
    // Still rendered, just hidden
}

// Use visible for frequent toggles (animations, etc.)
// Use @if for expensive components or when you want cleanup
```

---

## 8. List Rendering

### 8.1 For Loop

```liui
VBox {
    @for (item in state.items) {
        ListItem {
            key: ${item.id}
            text: ${item.name}
        }
    }
}

// With index
VBox {
    @for (item, index in state.items) {
        ListItem {
            key: ${item.id}
            text: ${(index + 1) + ". " + item.name}
        }
    }
}
```

### 8.2 Key Property

Keys help the framework efficiently update lists:

```liui
// Always use unique keys for lists
@for (user in state.users) {
    UserCard {
        key: ${user.id}  // Unique identifier
        name: ${user.name}
        email: ${user.email}
    }
}

// Bad: using index as key (causes issues with reordering)
@for (user, index in state.users) {
    UserCard {
        key: ${index}  // Don't do this!
    }
}
```

### 8.3 Filtered/Transformed Lists

```liui
// Filter in expression
@for (item in state.items.filter(i => i.active)) {
    ActiveItem { data: ${item} }
}

// Sort
@for (item in state.items.sortBy(i => i.name)) {
    ListItem { text: ${item.name} }
}

// Slice
@for (item in state.items.slice(0, 10)) {
    ListItem { text: ${item.name} }
}

// Complex transformation
@for (item in state.items
    .filter(i => i.category == state.filter)
    .sortBy(i => i.date)
    .reverse()) {
    ItemCard { data: ${item} }
}
```

### 8.4 Keyed Fragments

Render multiple elements per iteration:

```liui
@for (section in state.sections) {
    @fragment (key: ${section.id}) {
        SectionHeader { title: ${section.title} }

        @for (item in section.items) {
            SectionItem { key: ${item.id}, data: ${item} }
        }

        Divider { }
    }
}
```

---

## 9. Component Definition

### 9.1 Basic Component

```liui
// components/user_card.liui

component UserCard {
    // Props definition
    props {
        name: string
        email: string
        avatar: string?        // Optional
        role: string = "user"  // With default
    }

    // Component template
    Card {
        HBox {
            spacing: 12

            Image {
                src: ${props.avatar ?? "/default-avatar.png"}
                width: 48
                height: 48
                borderRadius: 24
            }

            VBox {
                Label {
                    text: ${props.name}
                    style: { fontWeight: "bold" }
                }
                Label {
                    text: ${props.email}
                    style: { color: "#666" }
                }
            }
        }
    }
}
```

### 9.2 Using Components

```liui
import { UserCard } from "./components/user_card.liui"

VBox {
    @for (user in state.users) {
        UserCard {
            key: ${user.id}
            name: ${user.name}
            email: ${user.email}
            avatar: ${user.avatarUrl}
        }
    }
}
```

### 9.3 Component State

```liui
component Counter {
    // Local state
    state {
        count: int = 0
    }

    // Methods
    fn increment() {
        state.count += 1
    }

    fn decrement() {
        state.count -= 1
    }

    // Template
    HBox {
        spacing: 8

        Button {
            text: "-"
            onClick: decrement
        }

        Label {
            text: ${state.count}
        }

        Button {
            text: "+"
            onClick: increment
        }
    }
}
```

### 9.4 Lifecycle Hooks

```liui
component DataFetcher {
    props {
        url: string
    }

    state {
        data: any? = null
        loading: bool = true
        error: string? = null
    }

    // Called when component mounts
    onMount {
        fetchData()
    }

    // Called when props change
    onUpdate(prev) {
        if (prev.url != props.url) {
            fetchData()
        }
    }

    // Called before component unmounts
    onUnmount {
        cancelFetch()
    }

    fn fetchData() {
        state.loading = true
        try {
            state.data = await fetch(props.url)
            state.error = null
        } catch (e) {
            state.error = e.message
        }
        state.loading = false
    }

    @if (state.loading) {
        Spinner { }
    } @else if (state.error) {
        ErrorMessage { text: ${state.error} }
    } @else {
        DataView { data: ${state.data} }
    }
}
```

### 9.5 Computed Properties

```liui
component ShoppingCart {
    props {
        items: List<CartItem>
    }

    // Computed properties (cached, updated reactively)
    computed {
        total: float => {
            props.items.sum(item => item.price * item.quantity)
        }

        itemCount: int => {
            props.items.sum(item => item.quantity)
        }

        isEmpty: bool => {
            props.items.length == 0
        }
    }

    VBox {
        @if (computed.isEmpty) {
            Label { text: "Your cart is empty" }
        } @else {
            @for (item in props.items) {
                CartItemRow { key: ${item.id}, item: ${item} }
            }

            Divider { }

            HBox {
                Label { text: ${"Items: " + computed.itemCount} }
                Spacer { }
                Label {
                    text: ${"Total: $" + computed.total.toFixed(2)}
                    style: { fontWeight: "bold" }
                }
            }
        }
    }
}
```

---

## 10. Slots and Children

### 10.1 Default Slot

```liui
// Define component with slot
component Card {
    props {
        title: string
    }

    VBox {
        style: {
            border: "1px solid #ddd"
            borderRadius: 8
            padding: 16
        }

        Label {
            text: ${props.title}
            style: { fontSize: 18, fontWeight: "bold" }
        }

        // Default slot renders children
        @slot

        // Or with fallback content
        @slot {
            Label { text: "No content provided" }
        }
    }
}

// Use component with children
Card {
    title: "My Card"

    Label { text: "This is the card content" }
    Button { text: "Action" }
}
```

### 10.2 Named Slots

```liui
// Define component with named slots
component Dialog {
    props {
        open: bool
    }

    @if (props.open) {
        Overlay {
            Panel {
                VBox {
                    // Header slot
                    @slot (name: "header") {
                        Label { text: "Dialog" }
                    }

                    // Default slot for body
                    @slot

                    // Footer slot
                    HBox {
                        justify: "end"
                        spacing: 8

                        @slot (name: "footer") {
                            Button { text: "Close" }
                        }
                    }
                }
            }
        }
    }
}

// Use with named slots
Dialog {
    open: ${state.showDialog}

    @slot (name: "header") {
        HBox {
            Label { text: "Confirm Action", style: { fontWeight: "bold" } }
            Spacer { }
            IconButton { icon: "close", onClick: () => state.showDialog = false }
        }
    }

    // Default slot content
    Label { text: "Are you sure you want to proceed?" }

    @slot (name: "footer") {
        Button { text: "Cancel", onClick: () => state.showDialog = false }
        Button { text: "Confirm", primary: true, onClick: confirm }
    }
}
```

### 10.3 Scoped Slots

Pass data to slot content:

```liui
// Define component with scoped slot
component DataList {
    props {
        items: List<T>
        renderItem: fn(T, int) -> Element  // Slot as prop
    }

    VBox {
        @for (item, index in props.items) {
            // Pass data to scoped slot
            ${props.renderItem(item, index)}
        }
    }
}

// Usage with scoped slot
DataList {
    items: ${state.users}

    renderItem: (user, index) => {
        UserCard {
            key: ${user.id}
            name: ${user.name}
            index: ${index}
        }
    }
}
```

---

## 11. Styling

### 11.1 Inline Styles

```liui
Label {
    text: "Styled Text"
    style: {
        color: "#333"
        fontSize: 16
        fontWeight: "bold"
        margin: 8
        padding: { top: 4, bottom: 4, left: 8, right: 8 }
    }
}
```

### 11.2 Style Objects

```liui
// Define styles at top of file or in state
const styles = {
    title: {
        fontSize: 24
        fontWeight: "bold"
        color: "#1a1a1a"
    }
    subtitle: {
        fontSize: 16
        color: "#666"
    }
    button: {
        padding: 12
        borderRadius: 6
        backgroundColor: "#007AFF"
    }
}

// Use styles
VBox {
    Label { text: "Title", style: styles.title }
    Label { text: "Subtitle", style: styles.subtitle }
    Button { text: "Action", style: styles.button }
}
```

### 11.3 Dynamic Styles

```liui
Button {
    text: "Dynamic Button"
    style: {
        backgroundColor: ${state.isActive ? "#4CAF50" : "#9E9E9E"}
        opacity: ${state.isEnabled ? 1.0 : 0.5}
        transform: ${"scale(" + (state.isPressed ? 0.95 : 1.0) + ")"}
    }
}

// Merge styles
Label {
    style: {
        ...baseStyle
        ...${state.isHighlighted ? highlightStyle : {}}
    }
}
```

### 11.4 Style Properties

Common style properties:

```liui
Element {
    style: {
        // Layout
        width: 100                // Fixed width
        height: "auto"            // Auto height
        minWidth: 50
        maxWidth: 500
        flex: 1                   // Flex grow
        flexShrink: 0

        // Spacing
        margin: 8                 // All sides
        marginTop: 16
        marginHorizontal: 12      // Left + right
        padding: { vertical: 8, horizontal: 16 }

        // Position
        position: "absolute"
        top: 0
        left: 0
        right: 0
        zIndex: 100

        // Colors
        backgroundColor: "#ffffff"
        color: "#333333"
        borderColor: "#ddd"

        // Border
        border: "1px solid #ddd"
        borderRadius: 8
        borderTopLeftRadius: 16

        // Typography
        fontSize: 16
        fontWeight: "bold"        // "normal", "bold", 100-900
        fontFamily: "system-ui"
        textAlign: "center"
        lineHeight: 1.5
        textDecoration: "underline"

        // Effects
        opacity: 0.8
        boxShadow: "0 2px 4px rgba(0,0,0,0.1)"
        transform: "rotate(45deg)"

        // Cursor
        cursor: "pointer"
    }
}
```

### 11.5 Themes

```liui
// theme.liui
export const theme = {
    colors: {
        primary: "#007AFF"
        secondary: "#5856D6"
        success: "#34C759"
        warning: "#FF9500"
        error: "#FF3B30"
        background: "#F2F2F7"
        surface: "#FFFFFF"
        text: "#1C1C1E"
        textSecondary: "#8E8E93"
    }
    spacing: {
        xs: 4
        sm: 8
        md: 16
        lg: 24
        xl: 32
    }
    typography: {
        h1: { fontSize: 32, fontWeight: "bold" }
        h2: { fontSize: 24, fontWeight: "bold" }
        body: { fontSize: 16, lineHeight: 1.5 }
        caption: { fontSize: 12, color: "#8E8E93" }
    }
    borderRadius: {
        sm: 4
        md: 8
        lg: 16
        full: 9999
    }
}

// Usage
import { theme } from "./theme.liui"

Button {
    style: {
        backgroundColor: theme.colors.primary
        padding: theme.spacing.md
        borderRadius: theme.borderRadius.md
    }
}
```

---

## 12. Animations

### 12.1 Transitions

```liui
Button {
    style: {
        backgroundColor: ${state.isHovered ? "#0056b3" : "#007AFF"}

        // Transition definition
        transition: {
            property: "backgroundColor"
            duration: 200        // ms
            easing: "ease-out"
        }
    }

    onMouseEnter: () => state.isHovered = true
    onMouseLeave: () => state.isHovered = false
}

// Multiple transitions
Panel {
    style: {
        opacity: ${state.visible ? 1 : 0}
        transform: ${state.visible ? "translateY(0)" : "translateY(-20px)"}

        transition: [
            { property: "opacity", duration: 300 }
            { property: "transform", duration: 300, easing: "ease-out" }
        ]
    }
}
```

### 12.2 Enter/Exit Animations

```liui
// Animate elements entering/leaving
@if (state.showModal) {
    Modal {
        @enter: {
            from: { opacity: 0, transform: "scale(0.9)" }
            to: { opacity: 1, transform: "scale(1)" }
            duration: 200
            easing: "ease-out"
        }
        @exit: {
            from: { opacity: 1, transform: "scale(1)" }
            to: { opacity: 0, transform: "scale(0.9)" }
            duration: 150
            easing: "ease-in"
        }

        // Modal content
    }
}

// List item animations
@for (item in state.items) {
    ListItem {
        key: ${item.id}

        @enter: {
            from: { opacity: 0, transform: "translateX(-20px)" }
            duration: 200
            delay: ${index * 50}  // Stagger
        }
        @exit: {
            to: { opacity: 0, transform: "translateX(20px)" }
            duration: 150
        }
    }
}
```

### 12.3 Keyframe Animations

```liui
// Define animation
const pulseAnimation = {
    keyframes: [
        { offset: 0, transform: "scale(1)" }
        { offset: 0.5, transform: "scale(1.05)" }
        { offset: 1, transform: "scale(1)" }
    ]
    duration: 1000
    iterations: "infinite"
}

// Use animation
Button {
    text: "Pulsing Button"
    animation: ${state.isPulsing ? pulseAnimation : null}
}

// Built-in animations
Spinner {
    animation: "spin"  // Built-in rotate animation
}
```

### 12.4 Spring Animations

```liui
// Physics-based spring animation
Panel {
    style: {
        transform: ${state.isOpen ? "translateX(0)" : "translateX(-100%)"}
    }

    transition: {
        property: "transform"
        spring: {
            stiffness: 300
            damping: 30
            mass: 1
        }
    }
}
```

---

## 13. Integration with Lira

### 13.1 State Management

```li
// src/app_state.li

pub class AppState {
    pub var count: int = 0
    pub var items: List<Item> = []
    pub var user: User? = null

    pub fn increment() {
        count += 1
    }

    pub fn addItem(item: Item) {
        items.push(item)
    }
}

pub let state = AppState()
```

```liui
// ui/app.liui
import { state } from "../src/app_state.li"

Window {
    VBox {
        Label { text: ${state.count} }
        Button {
            text: "Increment"
            onClick: state.increment
        }
    }
}
```

### 13.2 Calling Lira Functions

```li
// src/utils.li
pub fn formatCurrency(amount: float, currency: string) -> string {
    return currency + " " + amount.toFixed(2)
}

pub fn validateEmail(email: string) -> bool {
    return email.contains("@")
}
```

```liui
import { formatCurrency, validateEmail } from "../src/utils.li"

VBox {
    Label {
        text: ${formatCurrency(state.total, "USD")}
    }

    TextField {
        value: <=> state.email
        error: ${!validateEmail(state.email) ? "Invalid email" : null}
    }
}
```

### 13.3 Custom Components in Lira

```li
// src/components/chart.li
import gui.canvas.{Canvas, Context2D}

pub class ChartComponent {
    pub var data: List<float> = []
    pub var width: int = 200
    pub var height: int = 100

    pub fn render(ctx: Context2D) {
        // Custom drawing logic
        ctx.clearRect(0, 0, width, height)

        let barWidth = width / data.length
        for (i, value) in data.enumerate() {
            let barHeight = value / 100.0 * height
            ctx.fillRect(i * barWidth, height - barHeight, barWidth - 2, barHeight)
        }
    }
}
```

```liui
import { ChartComponent } from "../src/components/chart.li"

// Use custom component
Canvas {
    width: 400
    height: 200
    component: ChartComponent {
        data: ${state.chartData}
    }
}
```

### 13.4 Entry Point

```li
// src/main.li
import gui.core.App
import gui.window.Window

fn main() {
    let app = App.new("MyApp")

    // Load and run Lira UI file
    let window = Window.fromLiui("ui/main.liui")
    window.show()

    app.run()
}
```

---

## Appendix A: Lira UI Grammar

```
LiuiFile     ::= Import* ComponentDef* RootElement

Import       ::= 'import' ImportPath 'from' StringLiteral
             | 'import' '{' ImportList '}' 'from' StringLiteral

ComponentDef ::= 'component' Identifier '{' ComponentBody '}'
ComponentBody::= PropsBlock? StateBlock? ComputedBlock? LifecycleHook* Element

PropsBlock   ::= 'props' '{' PropDef* '}'
PropDef      ::= Identifier ':' Type ('=' Expression)?

StateBlock   ::= 'state' '{' StateDef* '}'
StateDef     ::= Identifier ':' Type '=' Expression

Element      ::= Identifier '{' ElementBody '}'
ElementBody  ::= (Property | Element | Directive)*

Property     ::= Identifier ':' Value
             | Identifier '.' Identifier ':' Value

Value        ::= Literal
             | ObjectLiteral
             | ArrayLiteral
             | BindExpr
             | TwoWayBind
             | Lambda

BindExpr     ::= '${' Expression '}'
TwoWayBind   ::= '<=>' Expression ('|' Modifier)*

Directive    ::= '@if' '(' Expression ')' '{' Element* '}'
                 ('@else' 'if' '(' Expression ')' '{' Element* '}')*
                 ('@else' '{' Element* '}')?
             | '@for' '(' Pattern 'in' Expression ')' '{' Element* '}'
             | '@when' '(' Expression ')' '{' WhenCase* '}'
             | '@slot' ('(' SlotArgs ')')? ('{' Element* '}')?
             | '@fragment' '(' FragmentArgs ')' '{' Element* '}'

WhenCase     ::= Pattern (Guard)? '=>' Element
Guard        ::= 'if' Expression
```

---

## Appendix B: Built-in Directives Reference

| Directive | Description |
|-----------|-------------|
| `@if` / `@else` | Conditional rendering |
| `@for` | List iteration |
| `@when` | Pattern matching |
| `@slot` | Slot placeholder |
| `@fragment` | Group elements without wrapper |
| `@enter` | Enter animation |
| `@exit` | Exit animation |

---

## Appendix C: Property Modifier Reference

| Modifier | Description |
|----------|-------------|
| `.stop` | Stop event propagation |
| `.prevent` | Prevent default behavior |
| `.capture` | Use capture phase |
| `.self` | Only trigger if target is self |
| `.once` | Trigger once then remove |
| `.passive` | Passive event listener |
| `.debounce(ms)` | Debounce value changes |
| `.throttle(ms)` | Throttle value changes |
| `.lazy` | Update on blur/submit |

---

*This document is part of the Lira Language Specification.*
