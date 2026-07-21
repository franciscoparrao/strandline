# Figure 1 — strandline pipeline flow diagram
# Computers & Geosciences, double-column (180 mm). Vector PDF + PNG mirror.
suppressPackageStartupMessages({
  library(ggplot2)
  library(systemfonts)
})

# --- fonts: Helvetica/Arial-equivalents present on Linux --------------------
sans_fam <- {
  fams <- unique(system_fonts()$family)
  cand <- c("Helvetica", "Arial", "Nimbus Sans", "Liberation Sans", "Arimo", "DejaVu Sans")
  cand[cand %in% fams][1]
}
mono_fam <- {
  fams <- unique(system_fonts()$family)
  cand <- c("Liberation Mono", "DejaVu Sans Mono", "Nimbus Mono PS", "Courier New", "mono")
  cand[cand %in% fams][1]
}

# --- palette (Wong, colourblind-safe): light fill / strong border ----------
pal <- list(
  input = list(fill = "#ECECEC", border = "#8A8A8A", text = "#1A1A1A"),
  extr  = list(fill = "#D9E7F2", border = "#0072B2", text = "#0A3A5C"),
  geom  = list(fill = "#D2F0E6", border = "#009E73", text = "#0A5340"),
  anly  = list(fill = "#FBEBCF", border = "#E69F00", text = "#6E4E00"),
  out   = list(fill = "#2E2E2E", border = "#2E2E2E", text = "#FFFFFF")
)

BW <- 0.90; BH <- 0.66; hw <- BW/2; hh <- BH/2   # box geometry
r1 <- 1.45; r2 <- 0.0                            # row y-centres

# --- boxes -----------------------------------------------------------------
box <- function(id, x, y, phase, title, detail = NA, cli = NA, dashed = FALSE)
  data.frame(id, x, y, phase, title, detail = detail, cli = cli, dashed = dashed)

boxes <- rbind(
  box("in",   1, r1, "input", "GeoTIFF / COG",        "multispectral scene"),
  box("mndwi",2, r1, "extr",  "Water index",          "MNDWI"),
  box("cls",  3, r1, "extr",  "Pixel classification",  "sand · water · whitewater · other", "classify"),
  box("otsu", 4, r1, "extr",  "Otsu threshold",        "class-balanced"),
  box("cont", 5, r1, "extr",  "Sub-pixel contour",     "marching squares → shoreline"),
  # row 2 (right -> left)
  box("tr",   5, r2, "geom",  "Shore-normal transects","smoothed baseline", "transects"),
  box("int",  4, r2, "geom",  "Vectorised intersection","shoreline × transects", "intersect"),
  box("rate", 2, r2, "anly",  "DSAS change rates",     "NSM · SCE · EPR · LRR · WLR", "rates"),
  box("qc",   1, r2, "anly",  "Quality control",       "5 criteria", "qc"),
  # optional branch + terminal
  box("tide", 3, -1.02, "anly", "Tidal correction",    "optional", "tide", dashed = TRUE)
)
out <- data.frame(x = 1, y = -1.02)

col <- function(p, k) vapply(p, function(z) pal[[z]][[k]], character(1))
boxes$fill <- col(boxes$phase, "fill"); boxes$bcol <- col(boxes$phase, "border"); boxes$tcol <- col(boxes$phase, "text")

# --- phase bands (behind) --------------------------------------------------
bands <- data.frame(
  xmin = c(1.55, 3.55, 0.55),
  xmax = c(5.45, 5.45, 3.45),
  ymin = c(r1-hh-0.13, r2-hh-0.13, -1.02-hh-0.13),
  ymax = c(r1+hh+0.30, r2+hh+0.54, r2+hh+0.54),
  hue  = c("#0072B2", "#009E73", "#E69F00"),
  lab  = c("Shoreline extraction  ·  strandline extract", "Transect geometry", "Change analysis")
)

# --- arrows ----------------------------------------------------------------
seg <- function(x, y, xend, yend, dashed = FALSE) data.frame(x, y, xend, yend, dashed)
arr <- rbind(
  # row 1 rightward
  seg(1+hw, r1, 2-hw, r1), seg(2+hw, r1, 3-hw, r1), seg(3+hw, r1, 4-hw, r1), seg(4+hw, r1, 5-hw, r1),
  # drop down at right
  seg(5, r1-hh, 5, r2+hh),
  # row 2 leftward (transects -> intersection -> [over tide] -> rates -> qc)
  seg(5-hw, r2, 4+hw, r2), seg(4-hw, r2, 2+hw, r2), seg(2-hw, r2, 1+hw, r2),
  # terminal down
  seg(1, r2-hh, 1, -1.02+hh)
)
arr_dash <- rbind(  # optional tidal detour
  seg(3.72, r2-hh, 3.28, -1.02+hh, TRUE),
  seg(2.72, -1.02+hh, 2.28, r2-hh, TRUE)
)

# --- plot ------------------------------------------------------------------
p <- ggplot() +
  geom_rect(data = bands, aes(xmin = xmin, xmax = xmax, ymin = ymin, ymax = ymax),
            fill = bands$hue, alpha = 0.08) +
  geom_text(data = bands, aes(x = xmin + 0.06, y = ymax - 0.13, label = lab),
            hjust = 0, vjust = 1, family = sans_fam, fontface = "bold",
            size = 2.15, colour = bands$hue) +
  # arrows
  geom_segment(data = arr, aes(x = x, y = y, xend = xend, yend = yend),
               linewidth = 0.34, colour = "#3A3A3A",
               arrow = arrow(length = unit(1.5, "mm"), type = "closed")) +
  geom_segment(data = arr_dash, aes(x = x, y = y, xend = xend, yend = yend),
               linewidth = 0.30, colour = "#9A6A00", linetype = "22",
               arrow = arrow(length = unit(1.3, "mm"), type = "closed")) +
  # boxes
  geom_rect(data = boxes, aes(xmin = x-hw, xmax = x+hw, ymin = y-hh, ymax = y+hh),
            fill = boxes$fill, colour = boxes$bcol,
            linewidth = ifelse(boxes$dashed, 0.5, 0.6),
            linetype = ifelse(boxes$dashed, "22", "solid")) +
  # terminal (output) pill
  geom_rect(data = out, aes(xmin = x-hw-0.02, xmax = x+hw+0.02, ymin = y-hh, ymax = y+hh),
            fill = pal$out$fill, colour = pal$out$border, linewidth = 0.6) +
  geom_text(data = out, aes(x = x, y = y), label = "Per-transect rates\n+ QC flags",
            family = sans_fam, fontface = "bold", size = 2.25, colour = pal$out$text, lineheight = 0.95) +
  # box text: title / detail / cli
  geom_text(data = boxes, aes(x = x, y = y + 0.135, label = title),
            family = sans_fam, fontface = "bold", size = 2.32, colour = boxes$tcol) +
  geom_text(data = subset(boxes, !is.na(detail)), aes(x = x, y = y - 0.02, label = detail),
            family = sans_fam, size = 1.92, colour = boxes$tcol[!is.na(boxes$detail)]) +
  geom_text(data = subset(boxes, !is.na(cli)), aes(x = x, y = y - 0.185, label = paste0("strandline ", cli)),
            family = mono_fam, size = 1.72, colour = "#6E6E6E") +
  coord_fixed(clip = "off", xlim = c(0.4, 5.6), ylim = c(-1.5, 2.05)) +
  theme_void() +
  theme(plot.margin = margin(2, 2, 2, 2))

# --- export: PDF + PNG mirror ----------------------------------------------
W <- 18.0; H <- W * (2.05 + 1.5) / (5.6 - 0.4)   # match coord aspect, no whitespace
fig_pdf <- "figures/pipeline.pdf"; fig_png <- "figures/pipeline.png"
ggsave(fig_pdf, p, width = W, height = H, units = "cm", device = cairo_pdf)
ggsave(fig_png, p, width = W, height = H, units = "cm", dpi = 300, device = ragg::agg_png)
cat(sprintf("wrote %s and %s  (%.1f x %.1f cm)\n", fig_pdf, fig_png, W, H))
