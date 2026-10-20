# Figure 3 — misalignment between fixed transects and the local shore-normal,
# measured on the ocean-anchored reanalysis (spike/transect_angle/ocean/).
# (a) persistent angle against its null, and against the same measurement on
#     the detection-derived transects of the original study, which inland water
#     had pulled off the coast; (b) per-site distribution: the misalignment is
#     small everywhere except at the Maule river mouth.
# Datos: ../spike/transect_angle/ocean/theta_{null,persist}.csv (SDS_DATASET=ocean)
#        ../spike/transect_angle/theta_persist.csv (conjunto original, agosto 2026)
suppressPackageStartupMessages({
  library(ggplot2); library(patchwork); library(systemfonts)
})

sans_fam <- {
  fams <- unique(system_fonts()$family)
  c("Helvetica", "Arial", "Nimbus Sans", "Liberation Sans", "Arimo",
    "DejaVu Sans")[c("Helvetica", "Arial", "Nimbus Sans", "Liberation Sans",
                     "Arimo", "DejaVu Sans") %in% fams][1]
}

D    <- "../spike/transect_angle"
null <- read.csv(file.path(D, "ocean", "theta_null.csv"))
per  <- read.csv(file.path(D, "ocean", "theta_persist.csv"))
orig <- read.csv(file.path(D, "theta_persist.csv"))

site_lv <- c("Viña del Mar", "Coquimbo", "Constitución", "Valdivia", "Chiloé")
per$site <- factor(per$site, levels = site_lv)

col_ocean <- "#0072B2"; col_orig <- "#D55E00"; grey_null <- "#9AA0A6"

theme_p <- theme_minimal(base_family = sans_fam, base_size = 8) +
  theme(
    panel.grid.minor = element_blank(),
    panel.grid.major = element_line(linewidth = 0.2, colour = "grey90"),
    axis.title = element_text(size = 7.5),
    axis.text  = element_text(size = 7, colour = "grey25"),
    plot.title = element_text(size = 8, face = "bold", hjust = 0),
    legend.position = "none",
    plot.margin = margin(3, 6, 3, 3)
  )

# --- (a) distribuciones: nula, transectos oceánicos, transectos originales -----
med_ocean <- median(per$theta_pers); med_orig <- median(orig$theta_pers)
lab_a <- data.frame(
  x = c(50, 50, 50),
  y = c(Inf, Inf, Inf), vj = c(1.3, 3.0, 6.8),
  col = c(col_ocean, col_orig, "grey40"),
  txt = c(sprintf("ocean-anchored transects\nmedian %.0f° (n = %d)", med_ocean, nrow(per)),
          sprintf("detection-derived transects\nmedian %.0f° (n = %d)", med_orig, nrow(orig)),
          "null: same estimator,\ntrue angle 0°"))
pa <- ggplot() +
  geom_density(data = null, aes(theta), fill = grey_null, colour = NA,
               alpha = 0.55, bw = 1.5) +
  geom_density(data = per, aes(theta_pers), fill = col_ocean, alpha = 0.30,
               colour = col_ocean, linewidth = 0.5, bw = 1.5) +
  geom_density(data = orig, aes(theta_pers), colour = col_orig, fill = NA,
               linewidth = 0.5, linetype = "22", bw = 5) +
  annotate("segment", x = c(med_ocean, med_orig), xend = c(med_ocean, med_orig),
           y = 0, yend = Inf, linetype = "22", linewidth = 0.3,
           colour = c(col_ocean, col_orig)) +
  geom_text(data = lab_a, aes(x, y, label = txt, vjust = vj), hjust = 0,
            colour = lab_a$col, family = sans_fam, size = 2.25, lineheight = 0.95) +
  scale_x_continuous(limits = c(0, 90), breaks = seq(0, 90, 15),
                     expand = expansion(mult = c(0.01, 0.01))) +
  scale_y_continuous(expand = expansion(mult = c(0, 0.05))) +
  labs(title = "a  Persistent misalignment",
       x = expression(theta[pers]~"(°)"), y = "density") +
  theme_p

# --- (b) por sitio, transectos oceánicos --------------------------------------
meds <- aggregate(theta_pers ~ site, per, median)
set.seed(1)
pb <- ggplot(per, aes(site, theta_pers)) +
  geom_jitter(width = 0.22, height = 0, size = 0.45, alpha = 0.35,
              colour = col_ocean) +
  geom_boxplot(width = 0.5, outlier.shape = NA, fill = NA, linewidth = 0.35,
               colour = "grey20") +
  geom_text(data = meds, aes(site, 88, label = sprintf("%.0f°", theta_pers)),
            family = sans_fam, size = 2.3, colour = "grey20") +
  scale_y_continuous(limits = c(0, 90), breaks = seq(0, 90, 15)) +
  labs(title = "b  By site (ocean-anchored)", x = NULL,
       y = expression(theta[pers]~"(°)")) +
  theme_p + theme(axis.text.x = element_text(size = 6.8))

fig <- pa | pb
W <- 18.0; H <- 6.8
ggsave("figures/transect_angle.pdf", fig, width = W, height = H,
       units = "cm", device = cairo_pdf)
ggsave("figures/transect_angle.png", fig, width = W, height = H,
       units = "cm", dpi = 300, device = ragg::agg_png)
cat(sprintf("wrote figures/transect_angle.{pdf,png} (%.1f x %.1f cm)\n", W, H))
