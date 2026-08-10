# Figure 3 — misalignment between fixed transects and the local shore-normal.
# Three panels: (a) how large the angle is against its null, (b) it is spatially
# coherent and decays with distance, (c) it is stable across disjoint halves of
# the record. Panels (b) and (c) are the discriminants: they separate coastal
# geometry from estimator noise.
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
null <- read.csv(file.path(D, "theta_null.csv"))
per  <- read.csv(file.path(D, "theta_persist.csv"))

site_lv <- c("Viña del Mar", "Coquimbo", "Constitución", "Valdivia")
per$site  <- factor(per$site,  levels = site_lv)
null$site <- factor(null$site, levels = site_lv)

# Wong palette, ordered north to south
pal <- c("Viña del Mar" = "#0072B2", "Coquimbo" = "#E69F00",
         "Constitución" = "#009E73", "Valdivia" = "#CC79A7")
grey_null <- "#9AA0A6"

theme_p <- theme_minimal(base_family = sans_fam, base_size = 8) +
  theme(
    panel.grid.minor = element_blank(),
    panel.grid.major = element_line(linewidth = 0.2, colour = "grey90"),
    axis.title = element_text(size = 7.5),
    axis.text  = element_text(size = 7, colour = "grey25"),
    plot.title = element_text(size = 8, face = "bold", hjust = 0),
    legend.position = "none",
    plot.margin = margin(3, 4, 3, 3)
  )

# --- (a) distribution of the persistent angle, against the null -------------
# Se agrupan los cuatro sitios: con n = 17 y 23 en Viña y Constitución, una
# densidad por sitio dibujaría modas que son ruido de muestreo, no estructura.
# Los sitios se separan en los paneles (b) y (c), donde hay n suficiente.
med_all <- median(per$theta_pers)
pa <- ggplot() +
  geom_density(data = null, aes(theta), fill = grey_null, colour = NA,
               alpha = 0.6, bw = 3) +
  geom_density(data = per, aes(theta_pers), fill = "#0072B2", alpha = 0.30,
               colour = "#0072B2", linewidth = 0.5, bw = 5) +
  annotate("segment", x = med_all, xend = med_all, y = 0, yend = Inf,
           linetype = "22", linewidth = 0.35, colour = "grey25") +
  annotate("text", x = med_all + 2.5, y = Inf, vjust = 1.7, hjust = 0,
           label = sprintf("median %.0f°\n(n = %d transects)", med_all, nrow(per)),
           family = sans_fam, size = 2.25, colour = "grey20", lineheight = 0.95) +
  annotate("text", x = 5.5, y = Inf, vjust = 1.7, hjust = 0,
           label = "null\n(θ = 0)", family = sans_fam, size = 2.25,
           colour = "grey40", lineheight = 0.95) +
  scale_x_continuous(limits = c(0, 90), breaks = seq(0, 90, 15),
                     expand = expansion(mult = c(0.01, 0.01))) +
  scale_y_continuous(expand = expansion(mult = c(0, 0.22))) +
  labs(title = "a  Persistent misalignment",
       x = expression(theta[pers]~"(°)"), y = "density") +
  theme_p

# --- (b) spatial coherence: r vs alongshore lag -----------------------------
lag_corr <- function(v, pos, lag) {
  idx <- match(pos + lag, pos)
  ok <- !is.na(idx)
  if (sum(ok) < 10) return(NA_real_)
  suppressWarnings(cor(v[ok], v[idx[ok]]))
}
lags <- 1:25
rows <- do.call(rbind, lapply(levels(per$site), function(s) {
  d <- per[per$site == s, ]
  if (nrow(d) < 30) return(NULL)
  d <- d[order(d$transect_num), ]
  data.frame(site = s, lag = lags,
             r = vapply(lags, function(L) lag_corr(d$theta_pers, d$transect_num, L), 0))
}))
rows$site <- factor(rows$site, levels = site_lv)

# permutation band (95%) pooled over the sites that qualify
set.seed(1)
nullr <- unlist(lapply(levels(per$site), function(s) {
  d <- per[per$site == s, ]
  if (nrow(d) < 30) return(NULL)
  d <- d[order(d$transect_num), ]
  replicate(400, lag_corr(sample(d$theta_pers), d$transect_num, 1))
}))
band <- quantile(nullr, c(0.025, 0.975), na.rm = TRUE)

pb <- ggplot(rows, aes(lag * 0.1, r, colour = site)) +
  annotate("rect", xmin = -Inf, xmax = Inf, ymin = band[1], ymax = band[2],
           fill = grey_null, alpha = 0.30) +
  annotate("text", x = 2.45, y = band[2], vjust = -0.6, hjust = 1,
           label = "permutation null (95%)", family = sans_fam,
           size = 2.2, colour = "grey35") +
  geom_hline(yintercept = 0, linewidth = 0.25, colour = "grey55") +
  geom_line(linewidth = 0.55) +
  geom_point(size = 0.7) +
  scale_colour_manual(values = pal, limits = c("Coquimbo", "Valdivia"),
                      name = NULL) +
  guides(colour = guide_legend(nrow = 1, override.aes = list(
    linewidth = 0.9, size = 1.4, alpha = 1))) +
  scale_x_continuous(expand = expansion(mult = c(0.02, 0.02))) +
  scale_y_continuous(limits = c(-0.25, 0.8)) +
  labs(title = "b  Spatially coherent, decaying",
       x = "alongshore separation (km)", y = expression("correlation of"~theta[pers])) +
  theme_p

# --- (c) temporal stability: two disjoint halves of the record --------------
h <- per[is.finite(per$theta_h1) & is.finite(per$theta_h2), ]
lab <- do.call(rbind, lapply(levels(per$site), function(s) {
  d <- h[h$site == s, ]
  if (nrow(d) < 30) return(NULL)
  data.frame(site = s, r = cor(d$theta_h1, d$theta_h2), n = nrow(d))
}))
lab$site <- factor(lab$site, levels = site_lv)
lab$lb <- sprintf("%s  r = %.2f", lab$site, lab$r)

pc <- ggplot(h[h$site %in% lab$site, ], aes(theta_h1, theta_h2, colour = site)) +
  geom_abline(slope = 1, intercept = 0, linewidth = 0.3, linetype = "22",
              colour = "grey55") +
  geom_point(size = 0.55, alpha = 0.55) +
  annotate("text", x = 2, y = c(86, 78), hjust = 0, vjust = 1,
           label = lab$lb[order(lab$site)], family = sans_fam, size = 2.3,
           colour = pal[as.character(lab$site[order(lab$site)])]) +
  scale_colour_manual(values = pal, limits = c("Coquimbo", "Valdivia"),
                      name = NULL) +
  coord_fixed(xlim = c(0, 90), ylim = c(0, 90)) +
  scale_x_continuous(breaks = seq(0, 90, 30)) +
  scale_y_continuous(breaks = seq(0, 90, 30)) +
  guides(colour = "none") +   # la leyenda la aporta (b); las claves de ambos
                              # geoms difieren y patchwork no las fundiría
  labs(title = "c  Stable in time",
       x = expression(theta[pers]~"first half (°)"),
       y = expression(theta[pers]~"second half (°)")) +
  theme_p

# leyenda compartida abajo. Sólo (b) y (c) usan color y comparten los mismos
# dos niveles, así que `guides = "collect"` las funde en una sola entrada.
fig <- (pa | pb | pc) +
  plot_layout(guides = "collect") &
  theme(legend.position = "bottom", legend.title = element_blank(),
        legend.key.size = unit(3.2, "mm"),
        legend.text = element_text(size = 7, family = sans_fam),
        legend.margin = margin(t = -2))

W <- 18.0; H <- 6.8
ggsave("figures/transect_angle.pdf", fig, width = W, height = H,
       units = "cm", device = cairo_pdf)
ggsave("figures/transect_angle.png", fig, width = W, height = H,
       units = "cm", dpi = 300, device = ragg::agg_png)
cat(sprintf("wrote figures/transect_angle.{pdf,png} (%.1f x %.1f cm)\n", W, H))
