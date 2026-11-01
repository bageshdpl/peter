PETER FOR MAC (ready to run, no installation)
=============================================
Works on Apple-chip (M1 and later) and Intel Macs, macOS 11 or later.

1. Start Peter (first time)
   Right-click (or Control-click) start-peter.command and choose "Open",
   then click "Open" again in the dialog. (A normal double-click is refused
   the first time because the file is not from the App Store.)
   A Terminal window opens (keep it open) and your browser opens Peter at
   http://localhost:5173. Create an account: it lives only on this Mac.
   After the first time, a normal double-click works.

   If macOS still says it cannot be opened: open System Settings, Privacy &
   Security, scroll down and click "Open Anyway" next to start-peter.command.

2. Add the Peter Observer extension to Chrome (once)
   - Go to chrome://extensions
   - Turn on "Developer mode" (top right)
   - Click "Load unpacked" and choose the  extension  folder in this folder
   - Pin "Peter Observer" from the puzzle-piece menu

3. Turn on AI drafting (optional)
   Follow the steps in  my-keys.example.sh  then start Peter again.
   (To edit it: right-click, Open With, TextEdit.)

4. Stop Peter
   Close the Terminal window, or press Ctrl+C in it.

Your data
   Everything is saved in this folder: peter.db and the peter-data folder.
   Back up those two to keep your work. Delete them to start fresh.
