# Wiki Source

The pages in this directory are the source of truth for the GitHub Wiki. The `Sync wiki` workflow mirrors this folder to the wiki repo when `wiki/**` changes on `main`.

Edit pages here, not on the GitHub web UI after bootstrap. Web edits are overwritten by the next sync.

`Home.md` is the landing page. `_Sidebar.md` controls the wiki sidebar.

## One-Time Bootstrap

The GitHub Wiki repo only exists after a maintainer creates the first page through the web UI. If the sync workflow says the wiki repo does not exist, create any placeholder page at:

<https://github.com/RealWhyKnot/ParsecToDreamcast/wiki>

Then re-run the workflow. The placeholder page is replaced by this directory on the next sync.
