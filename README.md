# 📚 PrepaWeek

<p align="center">
  <img src="assets/screenshot-main-dark.png" alt="Vue principale, mode sombre" width="45%">
  <img src="assets/screenshot-main-light.png" alt="Vue principale, mode clair" width="45%">
</p>
<p align="center">
  <img src="assets/screenshot-editor.png" alt="Éditeur de tâches intégré" width="45%">
  <img src="assets/screenshot-history.png" alt="Écran d'historique" width="45%">
</p>
<p align="center">
  <img src="assets/screenshot-confetti.png" alt="Semaine terminée à 100 % avec confettis" width="60%">
</p>

> **A simple desktop planner designed for students in preparatory classes.**

PrepaWeek is a lightweight desktop application that combines:

* 📅 Weekly schedule
* ✅ To-do list
* 📌 Task organization
* 💾 Local data saving

Built to help students keep track of classes, khôlles, homework, exams and personal tasks in a single place.

---

## ✨ Features

* 📆 Weekly timetable view
* ✅ Integrated to-do list
* 📌 Task management
* 💾 Automatic local saving
* ⚡ Fast and lightweight desktop application
* 🖥️ Native application (no browser required)
* 🎨 Simple and distraction-free interface

---

## 🖼️ Overview

```text id="0m0v3u"
┌──────────────────────────────────────┐
│              PrepaWeek               │
├──────────────────────────────────────┤
│ 📅 Weekly Schedule                   │
│                                      │
│ Monday    Tuesday    Wednesday ...   │
│                                      │
├──────────────────────────────────────┤
│ ✅ To-Do List                        │
│ □ Maths DM                           │
│ ☑ Physics exercises                  │
│ □ Learn English vocabulary           │
│ □ Prepare khôlle                     │
└──────────────────────────────────────┘
```

---

## 🎯 Why?

During *prépa*, information is often scattered:

* Homework
* Khôlles
* Exams
* Deadlines
* Weekly schedules
* Personal reminders

PrepaWeek aims to gather everything into one simple application:

> **One week. One app. Everything organized.**

---

## 🛠️ Technologies

* 🦀 Rust
* 🎨 egui / eframe
* ⚙️ Cargo
* 💾 Local configuration files

---

## 🚀 Installation

Clone the repository:

```bash id="m4hkk5"
git clone https://github.com/Astraheim/my-prepa-week-schedule-TDL.git
cd my-prepa-week-schedule-TDL
```

Build the project:

```bash id="1a3bpg"
cargo build --release
```

Run:

```bash id="zjynqz"
cargo run --release
```

The executable will be available in:

```text id="s9f9ci"
target/release/
```

---

## 📂 Project structure

```text id="1h1mou"
my-prepa-week-schedule-TDL/
│
├── src/
│   └── main.rs
│
├── Cargo.toml
├── Cargo.lock
├── build.rs
├── config.toml
├── README.md
│
└── target/        (ignored by Git)
```

---

## 💾 Data persistence

User preferences and tasks are saved locally.

This allows the application to remember:

* Tasks
* Settings
* Schedule data

without requiring an internet connection.

---

## 🔮 Future ideas

Some possible improvements:

* [ ] 📱 Mobile version
* [ ] ☁️ Synchronization
* [ ] 🔔 Notifications
* [ ] 📅 ICS import/export
* [ ] 📊 Statistics
* [ ] 🎨 Themes
* [ ] 📝 Notes
* [ ] ⏱️ Study timer (Pomodoro)
* [ ] 📚 Subject organization

---

## 📜 License

This project is mainly intended for personal use and experimentation.

Feel free to explore the code and adapt it to your own workflow.

---

<p align="center">

📚 Study less chaotically. Organize more efficiently.

</p>

