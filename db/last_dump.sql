/*M!999999\- enable the sandbox mode */ 
-- MariaDB dump 10.19  Distrib 10.6.19-MariaDB, for osx10.19 (arm64)
--
-- Host: 127.0.0.1    Database: obs_records
-- ------------------------------------------------------
-- Server version	10.6.19-MariaDB

/*!40101 SET @OLD_CHARACTER_SET_CLIENT=@@CHARACTER_SET_CLIENT */;
/*!40101 SET @OLD_CHARACTER_SET_RESULTS=@@CHARACTER_SET_RESULTS */;
/*!40101 SET @OLD_COLLATION_CONNECTION=@@COLLATION_CONNECTION */;
/*!40101 SET NAMES utf8mb4 */;
/*!40103 SET @OLD_TIME_ZONE=@@TIME_ZONE */;
/*!40103 SET TIME_ZONE='+00:00' */;
/*!40014 SET @OLD_UNIQUE_CHECKS=@@UNIQUE_CHECKS, UNIQUE_CHECKS=0 */;
/*!40014 SET @OLD_FOREIGN_KEY_CHECKS=@@FOREIGN_KEY_CHECKS, FOREIGN_KEY_CHECKS=0 */;
/*!40101 SET @OLD_SQL_MODE=@@SQL_MODE, SQL_MODE='NO_AUTO_VALUE_ON_ZERO' */;
/*!40111 SET @OLD_SQL_NOTES=@@SQL_NOTES, SQL_NOTES=0 */;

--
-- Table structure for table `api_status`
--

DROP TABLE IF EXISTS `api_status`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
/*!40101 SET character_set_client = utf8 */;
CREATE TABLE `api_status` (
  `status_id` tinyint(2) unsigned NOT NULL,
  `status_name` varchar(20) NOT NULL,
  PRIMARY KEY (`status_id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_general_ci;
/*!40101 SET character_set_client = @saved_cs_client */;

--
-- Table structure for table `api_status_history`
--

DROP TABLE IF EXISTS `api_status_history`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
/*!40101 SET character_set_client = utf8 */;
CREATE TABLE `api_status_history` (
  `status_history_id` int(11) unsigned NOT NULL AUTO_INCREMENT,
  `status_id` tinyint(2) unsigned NOT NULL,
  `status_history_date` datetime DEFAULT NULL,
  PRIMARY KEY (`status_history_id`,`status_id`),
  KEY `api_status_history__status_id` (`status_id`),
  CONSTRAINT `api_status_history___fk_status` FOREIGN KEY (`status_id`) REFERENCES `api_status` (`status_id`)
) ENGINE=InnoDB AUTO_INCREMENT=7 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_general_ci;
/*!40101 SET character_set_client = @saved_cs_client */;

--
-- Table structure for table `article`
--

DROP TABLE IF EXISTS `article`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
/*!40101 SET character_set_client = utf8 */;
CREATE TABLE `article` (
  `id` int(10) unsigned NOT NULL AUTO_INCREMENT,
  `slug` varchar(100) NOT NULL,
  `path` varchar(100) NOT NULL,
  `article_date` datetime NOT NULL,
  `hide` tinyint(1) DEFAULT NULL,
  PRIMARY KEY (`id`),
  UNIQUE KEY `article__uindex_slug` (`slug`)
) ENGINE=InnoDB AUTO_INCREMENT=3 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_general_ci;
/*!40101 SET character_set_client = @saved_cs_client */;

--
-- Table structure for table `article_authors`
--

DROP TABLE IF EXISTS `article_authors`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
/*!40101 SET character_set_client = utf8 */;
CREATE TABLE `article_authors` (
  `article_id` int(10) unsigned NOT NULL,
  `author_id` int(11) unsigned NOT NULL,
  PRIMARY KEY (`article_id`,`author_id`),
  KEY `article_authors___fk_author` (`author_id`),
  CONSTRAINT `article_authors___fk_article` FOREIGN KEY (`article_id`) REFERENCES `article` (`id`) ON DELETE CASCADE,
  CONSTRAINT `article_authors___fk_author` FOREIGN KEY (`author_id`) REFERENCES `players` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_general_ci;
/*!40101 SET character_set_client = @saved_cs_client */;

--
-- Table structure for table `banishments`
--

DROP TABLE IF EXISTS `banishments`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
/*!40101 SET character_set_client = utf8 */;
CREATE TABLE `banishments` (
  `id` int(11) unsigned NOT NULL AUTO_INCREMENT,
  `date_ban` datetime NOT NULL,
  `duration` bigint(20) DEFAULT NULL,
  `was_reprieved` tinyint(1) NOT NULL,
  `reason` varchar(128) NOT NULL DEFAULT '',
  `player_id` int(11) unsigned DEFAULT NULL,
  `banished_by` int(11) unsigned DEFAULT NULL,
  PRIMARY KEY (`id`),
  KEY `player_id` (`player_id`),
  KEY `banished_by` (`banished_by`),
  CONSTRAINT `banishments___fk_author` FOREIGN KEY (`banished_by`) REFERENCES `players` (`id`) ON DELETE SET NULL,
  CONSTRAINT `banishments___fk_player` FOREIGN KEY (`player_id`) REFERENCES `players` (`id`) ON DELETE SET NULL
) ENGINE=InnoDB AUTO_INCREMENT=39 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_general_ci;
/*!40101 SET character_set_client = @saved_cs_client */;
/*!50003 SET @saved_cs_client      = @@character_set_client */ ;
/*!50003 SET @saved_cs_results     = @@character_set_results */ ;
/*!50003 SET @saved_col_connection = @@collation_connection */ ;
/*!50003 SET character_set_client  = utf8mb4 */ ;
/*!50003 SET character_set_results = utf8mb4 */ ;
/*!50003 SET collation_connection  = utf8mb4_general_ci */ ;
/*!50003 SET @saved_sql_mode       = @@sql_mode */ ;
/*!50003 SET sql_mode              = 'IGNORE_SPACE,STRICT_TRANS_TABLES,ERROR_FOR_DIVISION_BY_ZERO,NO_AUTO_CREATE_USER,NO_ENGINE_SUBSTITUTION' */ ;
DELIMITER ;;
/*!50003 CREATE*/ /*!50017 DEFINER=`root`@`localhost`*/ /*!50003 trigger already_banned
    before insert
    on banishments
    for each row
    if
	(select count(*)
		from banishments
		where player_id = new.player_id
			and (date_ban + interval duration second > now() or duration = -1))
	> 0 then
	signal sqlstate '45000'
	set message_text = 'player is already banned';
end if */;;
DELIMITER ;
/*!50003 SET sql_mode              = @saved_sql_mode */ ;
/*!50003 SET character_set_client  = @saved_cs_client */ ;
/*!50003 SET character_set_results = @saved_cs_results */ ;
/*!50003 SET collation_connection  = @saved_col_connection */ ;

--
-- Table structure for table `checkpoint_times`
--

DROP TABLE IF EXISTS `checkpoint_times`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
/*!40101 SET character_set_client = utf8 */;
CREATE TABLE `checkpoint_times` (
  `cp_num` int(3) unsigned NOT NULL,
  `map_id` int(11) unsigned NOT NULL,
  `record_id` int(10) unsigned NOT NULL,
  `time` int(11) NOT NULL,
  PRIMARY KEY (`cp_num`,`map_id`,`record_id`),
  KEY `record_id` (`record_id`),
  CONSTRAINT `checkpoint_times___fk_record` FOREIGN KEY (`record_id`) REFERENCES `records` (`record_id`) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_general_ci;
/*!40101 SET character_set_client = @saved_cs_client */;

--
-- Temporary table structure for view `current_bans`
--

DROP TABLE IF EXISTS `current_bans`;
/*!50001 DROP VIEW IF EXISTS `current_bans`*/;
SET @saved_cs_client     = @@character_set_client;
SET character_set_client = utf8;
/*!50001 CREATE VIEW `current_bans` AS SELECT
 1 AS `id`,
  1 AS `date_ban`,
  1 AS `duration`,
  1 AS `was_reprieved`,
  1 AS `reason`,
  1 AS `player_id`,
  1 AS `banished_by`,
  1 AS `remaining_secs` */;
SET character_set_client = @saved_cs_client;

--
-- Table structure for table `event`
--

DROP TABLE IF EXISTS `event`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
/*!40101 SET character_set_client = utf8 */;
CREATE TABLE `event` (
  `id` int(11) unsigned NOT NULL AUTO_INCREMENT,
  `handle` varchar(32) NOT NULL DEFAULT '',
  `cooldown` int(8) unsigned DEFAULT NULL,
  PRIMARY KEY (`id`),
  UNIQUE KEY `event_handle_index` (`handle`)
) ENGINE=InnoDB AUTO_INCREMENT=12 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_general_ci;
/*!40101 SET character_set_client = @saved_cs_client */;

--
-- Table structure for table `event_admins`
--

DROP TABLE IF EXISTS `event_admins`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
/*!40101 SET character_set_client = utf8 */;
CREATE TABLE `event_admins` (
  `event_id` int(11) unsigned NOT NULL,
  `player_id` int(11) unsigned NOT NULL,
  PRIMARY KEY (`event_id`,`player_id`),
  KEY `event_admins___fk_player` (`player_id`),
  CONSTRAINT `event_admins___fk_event` FOREIGN KEY (`event_id`) REFERENCES `event` (`id`),
  CONSTRAINT `event_admins___fk_player` FOREIGN KEY (`player_id`) REFERENCES `players` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_general_ci;
/*!40101 SET character_set_client = @saved_cs_client */;

--
-- Table structure for table `event_categories`
--

DROP TABLE IF EXISTS `event_categories`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
/*!40101 SET character_set_client = utf8 */;
CREATE TABLE `event_categories` (
  `event_id` int(11) unsigned NOT NULL,
  `category_id` int(11) unsigned NOT NULL,
  PRIMARY KEY (`event_id`,`category_id`),
  KEY `category_id` (`category_id`),
  CONSTRAINT `event_categories___fk_category` FOREIGN KEY (`category_id`) REFERENCES `event_category` (`id`) ON DELETE CASCADE,
  CONSTRAINT `event_categories___fk_event` FOREIGN KEY (`event_id`) REFERENCES `event` (`id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_general_ci;
/*!40101 SET character_set_client = @saved_cs_client */;

--
-- Table structure for table `event_category`
--

DROP TABLE IF EXISTS `event_category`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
/*!40101 SET character_set_client = utf8 */;
CREATE TABLE `event_category` (
  `id` int(11) unsigned NOT NULL AUTO_INCREMENT,
  `handle` varchar(32) NOT NULL DEFAULT '',
  `name` varchar(128) NOT NULL DEFAULT '',
  `banner_img_url` varchar(128) DEFAULT NULL,
  `hex_color` varchar(8) DEFAULT NULL,
  PRIMARY KEY (`id`)
) ENGINE=InnoDB AUTO_INCREMENT=28 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_general_ci;
/*!40101 SET character_set_client = @saved_cs_client */;

--
-- Table structure for table `event_edition`
--

DROP TABLE IF EXISTS `event_edition`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
/*!40101 SET character_set_client = utf8 */;
CREATE TABLE `event_edition` (
  `id` int(11) unsigned NOT NULL AUTO_INCREMENT,
  `event_id` int(11) unsigned NOT NULL,
  `name` varchar(255) NOT NULL,
  `subtitle` varchar(255) DEFAULT NULL,
  `start_date` datetime NOT NULL,
  `banner_img_url` varchar(512) DEFAULT NULL,
  `banner2_img_url` varchar(512) DEFAULT NULL,
  `mx_id` int(11) DEFAULT NULL,
  `mx_secret` varchar(20) DEFAULT NULL,
  `ttl` int(5) unsigned DEFAULT NULL,
  `save_non_event_record` tinyint(1) NOT NULL DEFAULT 1,
  `non_original_maps` tinyint(1) NOT NULL DEFAULT 1,
  PRIMARY KEY (`id`,`event_id`),
  KEY `event_id` (`event_id`),
  KEY `event_edition_index_mx_id` (`mx_id`),
  CONSTRAINT `event_edition___fk_event` FOREIGN KEY (`event_id`) REFERENCES `event` (`id`)
) ENGINE=InnoDB AUTO_INCREMENT=10 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_general_ci;
/*!40101 SET character_set_client = @saved_cs_client */;

--
-- Table structure for table `event_edition_admins`
--

DROP TABLE IF EXISTS `event_edition_admins`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
/*!40101 SET character_set_client = utf8 */;
CREATE TABLE `event_edition_admins` (
  `event_id` int(10) unsigned NOT NULL,
  `edition_id` int(10) unsigned NOT NULL,
  `player_id` int(11) unsigned NOT NULL,
  PRIMARY KEY (`event_id`,`edition_id`,`player_id`),
  KEY `event_edition_admins___fk_player` (`player_id`),
  CONSTRAINT `event_edition_admins___fk_edition` FOREIGN KEY (`event_id`, `edition_id`) REFERENCES `event_edition` (`event_id`, `id`) ON UPDATE CASCADE,
  CONSTRAINT `event_edition_admins___fk_player` FOREIGN KEY (`player_id`) REFERENCES `players` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_general_ci;
/*!40101 SET character_set_client = @saved_cs_client */;

--
-- Table structure for table `event_edition_categories`
--

DROP TABLE IF EXISTS `event_edition_categories`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
/*!40101 SET character_set_client = utf8 */;
CREATE TABLE `event_edition_categories` (
  `event_id` int(10) unsigned NOT NULL,
  `edition_id` int(11) unsigned NOT NULL,
  `category_id` int(11) unsigned NOT NULL,
  PRIMARY KEY (`event_id`,`category_id`,`edition_id`),
  KEY `category_id` (`category_id`),
  KEY `event_edition_categories___fk_edition` (`event_id`,`edition_id`),
  CONSTRAINT `event_edition_categories___fk_category` FOREIGN KEY (`category_id`) REFERENCES `event_category` (`id`) ON DELETE CASCADE,
  CONSTRAINT `event_edition_categories___fk_edition` FOREIGN KEY (`event_id`, `edition_id`) REFERENCES `event_edition` (`event_id`, `id`) ON UPDATE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_general_ci;
/*!40101 SET character_set_client = @saved_cs_client */;
/*!50003 SET @saved_cs_client      = @@character_set_client */ ;
/*!50003 SET @saved_cs_results     = @@character_set_results */ ;
/*!50003 SET @saved_col_connection = @@collation_connection */ ;
/*!50003 SET character_set_client  = utf8mb4 */ ;
/*!50003 SET character_set_results = utf8mb4 */ ;
/*!50003 SET collation_connection  = utf8mb4_general_ci */ ;
/*!50003 SET @saved_sql_mode       = @@sql_mode */ ;
/*!50003 SET sql_mode              = 'IGNORE_SPACE,STRICT_TRANS_TABLES,ERROR_FOR_DIVISION_BY_ZERO,NO_AUTO_CREATE_USER,NO_ENGINE_SUBSTITUTION' */ ;
DELIMITER ;;
/*!50003 CREATE*/ /*!50017 DEFINER=`root`@`localhost`*/ /*!50003 trigger event_edition_categories___t_updater
    after delete
    on event_edition_categories
    for each row
begin
    update event_edition_maps set category_id = null
    where event_id = old.event_id and edition_id = old.edition_id and category_id = old.category_id;
end */;;
DELIMITER ;
/*!50003 SET sql_mode              = @saved_sql_mode */ ;
/*!50003 SET character_set_client  = @saved_cs_client */ ;
/*!50003 SET character_set_results = @saved_cs_results */ ;
/*!50003 SET collation_connection  = @saved_col_connection */ ;

--
-- Table structure for table `event_edition_maps`
--

DROP TABLE IF EXISTS `event_edition_maps`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
/*!40101 SET character_set_client = utf8 */;
CREATE TABLE `event_edition_maps` (
  `event_id` int(11) unsigned NOT NULL,
  `edition_id` int(11) unsigned NOT NULL,
  `map_id` int(11) unsigned NOT NULL,
  `category_id` int(11) unsigned DEFAULT NULL,
  `mx_id` bigint(20) DEFAULT NULL,
  `order` int(10) unsigned NOT NULL,
  `original_map_id` int(11) unsigned DEFAULT NULL,
  `original_mx_id` bigint(20) DEFAULT NULL,
  `transitive_save` tinyint(1) DEFAULT NULL,
  `bronze_time` int(11) DEFAULT NULL,
  `silver_time` int(11) DEFAULT NULL,
  `gold_time` int(11) DEFAULT NULL,
  `author_time` int(11) DEFAULT NULL,
  PRIMARY KEY (`event_id`,`map_id`,`edition_id`),
  KEY `map_id` (`map_id`),
  KEY `event_edition_maps_category` (`category_id`),
  KEY `event_edition_maps_event_edition` (`edition_id`,`event_id`),
  KEY `event_edition_maps___fk_category` (`event_id`,`edition_id`,`category_id`),
  KEY `event_edition_maps___fk_original_map` (`original_map_id`),
  CONSTRAINT `event_edition_maps___fk_category` FOREIGN KEY (`event_id`, `edition_id`, `category_id`) REFERENCES `event_edition_categories` (`event_id`, `edition_id`, `category_id`),
  CONSTRAINT `event_edition_maps___fk_edition` FOREIGN KEY (`event_id`, `edition_id`) REFERENCES `event_edition` (`event_id`, `id`) ON UPDATE CASCADE,
  CONSTRAINT `event_edition_maps___fk_map` FOREIGN KEY (`map_id`) REFERENCES `maps` (`id`) ON DELETE CASCADE,
  CONSTRAINT `event_edition_maps___fk_original_map` FOREIGN KEY (`original_map_id`) REFERENCES `maps` (`id`) ON DELETE SET NULL
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_general_ci;
/*!40101 SET character_set_client = @saved_cs_client */;

--
-- Table structure for table `event_edition_records`
--

DROP TABLE IF EXISTS `event_edition_records`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
/*!40101 SET character_set_client = utf8 */;
CREATE TABLE `event_edition_records` (
  `record_id` int(11) unsigned NOT NULL,
  `event_id` int(11) unsigned NOT NULL,
  `edition_id` int(11) unsigned NOT NULL,
  PRIMARY KEY (`record_id`),
  KEY `event_edition_records_event_edition` (`event_id`,`edition_id`),
  CONSTRAINT `event_edition_records___fk_edition` FOREIGN KEY (`event_id`, `edition_id`) REFERENCES `event_edition` (`event_id`, `id`) ON UPDATE CASCADE,
  CONSTRAINT `event_edition_records___fk_record` FOREIGN KEY (`record_id`) REFERENCES `records` (`record_id`) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_general_ci;
/*!40101 SET character_set_client = @saved_cs_client */;

--
-- Temporary table structure for view `global_event_records`
--

DROP TABLE IF EXISTS `global_event_records`;
/*!50001 DROP VIEW IF EXISTS `global_event_records`*/;
SET @saved_cs_client     = @@character_set_client;
SET character_set_client = utf8;
/*!50001 CREATE VIEW `global_event_records` AS SELECT
 1 AS `record_id`,
  1 AS `record_player_id`,
  1 AS `map_id`,
  1 AS `time`,
  1 AS `respawn_count`,
  1 AS `record_date`,
  1 AS `flags`,
  1 AS `try_count`,
  1 AS `event_record_id`,
  1 AS `event_id`,
  1 AS `edition_id` */;
SET character_set_client = @saved_cs_client;

--
-- Temporary table structure for view `global_records`
--

DROP TABLE IF EXISTS `global_records`;
/*!50001 DROP VIEW IF EXISTS `global_records`*/;
SET @saved_cs_client     = @@character_set_client;
SET character_set_client = utf8;
/*!50001 CREATE VIEW `global_records` AS SELECT
 1 AS `record_id`,
  1 AS `record_player_id`,
  1 AS `map_id`,
  1 AS `time`,
  1 AS `respawn_count`,
  1 AS `record_date`,
  1 AS `flags`,
  1 AS `try_count`,
  1 AS `event_record_id` */;
SET character_set_client = @saved_cs_client;

--
-- Table structure for table `latestnews_image`
--

DROP TABLE IF EXISTS `latestnews_image`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
/*!40101 SET character_set_client = utf8 */;
CREATE TABLE `latestnews_image` (
  `id` int(5) NOT NULL AUTO_INCREMENT,
  `img_url` varchar(512) NOT NULL,
  `link` varchar(512) NOT NULL,
  PRIMARY KEY (`id`)
) ENGINE=InnoDB AUTO_INCREMENT=2 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_general_ci;
/*!40101 SET character_set_client = @saved_cs_client */;

--
-- Table structure for table `maps`
--

DROP TABLE IF EXISTS `maps`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
/*!40101 SET character_set_client = utf8 */;
CREATE TABLE `maps` (
  `id` int(10) unsigned NOT NULL AUTO_INCREMENT,
  `game_id` varchar(255) CHARACTER SET utf8mb4 COLLATE utf8mb4_general_ci NOT NULL,
  `player_id` int(10) unsigned NOT NULL,
  `name` varchar(512) NOT NULL DEFAULT '',
  `cps_number` int(5) unsigned DEFAULT NULL,
  `linked_map` int(11) unsigned DEFAULT NULL,
  PRIMARY KEY (`id`),
  UNIQUE KEY `maniaplanet_map_id` (`game_id`) USING BTREE,
  KEY `FK_maps_players` (`player_id`),
  KEY `maps_linked_map_fk` (`linked_map`),
  CONSTRAINT `maps___fk_author` FOREIGN KEY (`player_id`) REFERENCES `players` (`id`),
  CONSTRAINT `maps___fk_linked_map` FOREIGN KEY (`linked_map`) REFERENCES `maps` (`id`) ON DELETE SET NULL
) ENGINE=InnoDB AUTO_INCREMENT=86837 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
/*!40101 SET character_set_client = @saved_cs_client */;

--
-- Table structure for table `player_rating`
--

DROP TABLE IF EXISTS `player_rating`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
/*!40101 SET character_set_client = utf8 */;
CREATE TABLE `player_rating` (
  `player_id` int(11) unsigned NOT NULL,
  `map_id` int(11) unsigned NOT NULL,
  `kind` tinyint(1) unsigned NOT NULL,
  `rating` float unsigned NOT NULL,
  PRIMARY KEY (`player_id`,`map_id`,`kind`),
  KEY `kind` (`kind`),
  KEY `player_rating___fk_map` (`map_id`),
  CONSTRAINT `player_rating___fk_kind` FOREIGN KEY (`kind`) REFERENCES `rating_kind` (`id`) ON DELETE CASCADE,
  CONSTRAINT `player_rating___fk_map` FOREIGN KEY (`map_id`) REFERENCES `maps` (`id`) ON DELETE CASCADE,
  CONSTRAINT `player_rating___fk_player` FOREIGN KEY (`player_id`) REFERENCES `players` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_general_ci;
/*!40101 SET character_set_client = @saved_cs_client */;
/*!50003 SET @saved_cs_client      = @@character_set_client */ ;
/*!50003 SET @saved_cs_results     = @@character_set_results */ ;
/*!50003 SET @saved_col_connection = @@collation_connection */ ;
/*!50003 SET character_set_client  = utf8mb4 */ ;
/*!50003 SET character_set_results = utf8mb4 */ ;
/*!50003 SET collation_connection  = utf8mb4_general_ci */ ;
/*!50003 SET @saved_sql_mode       = @@sql_mode */ ;
/*!50003 SET sql_mode              = 'STRICT_TRANS_TABLES,ERROR_FOR_DIVISION_BY_ZERO,NO_AUTO_CREATE_USER,NO_ENGINE_SUBSTITUTION' */ ;
DELIMITER ;;
/*!50003 CREATE*/ /*!50017 DEFINER=`records_api`@`localhost`*/ /*!50003 TRIGGER `player_rating_ins` BEFORE INSERT ON `player_rating` FOR EACH ROW if new.rating > 1 or new.rating < 0 then
	signal sqlstate '45000' set message_text = 'rating field is not valid';
end if */;;
DELIMITER ;
/*!50003 SET sql_mode              = @saved_sql_mode */ ;
/*!50003 SET character_set_client  = @saved_cs_client */ ;
/*!50003 SET character_set_results = @saved_cs_results */ ;
/*!50003 SET collation_connection  = @saved_col_connection */ ;
/*!50003 SET @saved_cs_client      = @@character_set_client */ ;
/*!50003 SET @saved_cs_results     = @@character_set_results */ ;
/*!50003 SET @saved_col_connection = @@collation_connection */ ;
/*!50003 SET character_set_client  = utf8mb4 */ ;
/*!50003 SET character_set_results = utf8mb4 */ ;
/*!50003 SET collation_connection  = utf8mb4_general_ci */ ;
/*!50003 SET @saved_sql_mode       = @@sql_mode */ ;
/*!50003 SET sql_mode              = 'STRICT_TRANS_TABLES,ERROR_FOR_DIVISION_BY_ZERO,NO_AUTO_CREATE_USER,NO_ENGINE_SUBSTITUTION' */ ;
DELIMITER ;;
/*!50003 CREATE*/ /*!50017 DEFINER=`records_api`@`localhost`*/ /*!50003 TRIGGER `player_rating_upd` BEFORE UPDATE ON `player_rating` FOR EACH ROW if new.rating > 1 or new.rating < 0 then
	signal sqlstate '45000' set message_text = 'rating field is not valid';
end if */;;
DELIMITER ;
/*!50003 SET sql_mode              = @saved_sql_mode */ ;
/*!50003 SET character_set_client  = @saved_cs_client */ ;
/*!50003 SET character_set_results = @saved_cs_results */ ;
/*!50003 SET collation_connection  = @saved_col_connection */ ;

--
-- Table structure for table `players`
--

DROP TABLE IF EXISTS `players`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
/*!40101 SET character_set_client = utf8 */;
CREATE TABLE `players` (
  `id` int(10) unsigned NOT NULL AUTO_INCREMENT,
  `login` varchar(255) CHARACTER SET utf8mb4 COLLATE utf8mb4_general_ci NOT NULL,
  `name` varchar(512) NOT NULL DEFAULT '',
  `join_date` datetime DEFAULT NULL,
  `zone_path` varchar(128) CHARACTER SET utf8mb4 COLLATE utf8mb4_general_ci DEFAULT '',
  `admins_note` varchar(128) CHARACTER SET utf8mb4 COLLATE utf8mb4_general_ci DEFAULT NULL,
  `role` tinyint(1) unsigned NOT NULL,
  PRIMARY KEY (`id`),
  UNIQUE KEY `login` (`login`),
  KEY `role` (`role`),
  CONSTRAINT `players___fk_role` FOREIGN KEY (`role`) REFERENCES `role` (`id`)
) ENGINE=InnoDB AUTO_INCREMENT=34743 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
/*!40101 SET character_set_client = @saved_cs_client */;

--
-- Table structure for table `players_ips`
--

DROP TABLE IF EXISTS `players_ips`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
/*!40101 SET character_set_client = utf8 */;
CREATE TABLE `players_ips` (
  `player_id` int(11) unsigned NOT NULL,
  `ip_hash` binary(32) NOT NULL DEFAULT '\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0',
  `ip_date` datetime NOT NULL,
  PRIMARY KEY (`player_id`,`ip_hash`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_general_ci;
/*!40101 SET character_set_client = @saved_cs_client */;

--
-- Table structure for table `rating`
--

DROP TABLE IF EXISTS `rating`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
/*!40101 SET character_set_client = utf8 */;
CREATE TABLE `rating` (
  `player_id` int(11) unsigned NOT NULL,
  `map_id` int(11) unsigned NOT NULL,
  `rating_date` datetime NOT NULL,
  PRIMARY KEY (`player_id`,`map_id`),
  KEY `map_id` (`map_id`),
  CONSTRAINT `rating___fk_map` FOREIGN KEY (`map_id`) REFERENCES `maps` (`id`) ON DELETE CASCADE,
  CONSTRAINT `rating___fk_player` FOREIGN KEY (`player_id`) REFERENCES `players` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_general_ci;
/*!40101 SET character_set_client = @saved_cs_client */;

--
-- Table structure for table `rating_kind`
--

DROP TABLE IF EXISTS `rating_kind`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
/*!40101 SET character_set_client = utf8 */;
CREATE TABLE `rating_kind` (
  `id` tinyint(1) unsigned NOT NULL,
  `kind` varchar(10) NOT NULL DEFAULT '',
  PRIMARY KEY (`id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_general_ci;
/*!40101 SET character_set_client = @saved_cs_client */;

--
-- Table structure for table `records`
--

DROP TABLE IF EXISTS `records`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
/*!40101 SET character_set_client = utf8 */;
CREATE TABLE `records` (
  `record_id` int(10) unsigned NOT NULL AUTO_INCREMENT,
  `record_player_id` int(11) unsigned NOT NULL,
  `map_id` int(11) unsigned NOT NULL,
  `time` int(11) NOT NULL,
  `respawn_count` int(11) NOT NULL,
  `record_date` datetime NOT NULL,
  `flags` int(11) unsigned NOT NULL DEFAULT 0,
  `try_count` int(11) unsigned DEFAULT NULL,
  `event_record_id` int(10) unsigned DEFAULT NULL,
  PRIMARY KEY (`record_id`),
  KEY `FK_records_players` (`record_player_id`),
  KEY `FK_records_maps` (`map_id`),
  KEY `time` (`time`),
  KEY `records_record_date_index` (`record_date`),
  KEY `idx_records_on_player_map_date` (`record_player_id`,`map_id`,`record_date`),
  KEY `records___fk_event_record` (`event_record_id`),
  CONSTRAINT `records___fk_event_record` FOREIGN KEY (`event_record_id`) REFERENCES `records` (`record_id`) ON DELETE SET NULL,
  CONSTRAINT `records___fk_map` FOREIGN KEY (`map_id`) REFERENCES `maps` (`id`) ON DELETE CASCADE,
  CONSTRAINT `records___fk_player` FOREIGN KEY (`record_player_id`) REFERENCES `players` (`id`)
) ENGINE=InnoDB AUTO_INCREMENT=815087 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_general_ci;
/*!40101 SET character_set_client = @saved_cs_client */;

--
-- Table structure for table `resources_content`
--

DROP TABLE IF EXISTS `resources_content`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
/*!40101 SET character_set_client = utf8 */;
CREATE TABLE `resources_content` (
  `id` int(5) NOT NULL AUTO_INCREMENT,
  `content` mediumtext NOT NULL,
  `created_at` datetime DEFAULT NULL,
  PRIMARY KEY (`id`)
) ENGINE=InnoDB AUTO_INCREMENT=2 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_general_ci;
/*!40101 SET character_set_client = @saved_cs_client */;

--
-- Table structure for table `role`
--

DROP TABLE IF EXISTS `role`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
/*!40101 SET character_set_client = utf8 */;
CREATE TABLE `role` (
  `id` tinyint(1) unsigned NOT NULL,
  `role_name` varchar(6) NOT NULL DEFAULT '',
  `privileges` tinyint(3) unsigned DEFAULT NULL,
  PRIMARY KEY (`id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_general_ci;
/*!40101 SET character_set_client = @saved_cs_client */;

--
-- Final view structure for view `current_bans`
--

/*!50001 DROP VIEW IF EXISTS `current_bans`*/;
/*!50001 SET @saved_cs_client          = @@character_set_client */;
/*!50001 SET @saved_cs_results         = @@character_set_results */;
/*!50001 SET @saved_col_connection     = @@collation_connection */;
/*!50001 SET character_set_client      = utf8mb4 */;
/*!50001 SET character_set_results     = utf8mb4 */;
/*!50001 SET collation_connection      = utf8mb4_general_ci */;
/*!50001 CREATE ALGORITHM=UNDEFINED */
/*!50013 DEFINER=`root`@`localhost` SQL SECURITY DEFINER */
/*!50001 VIEW `current_bans` AS select `banishments`.`id` AS `id`,`banishments`.`date_ban` AS `date_ban`,`banishments`.`duration` AS `duration`,`banishments`.`was_reprieved` AS `was_reprieved`,`banishments`.`reason` AS `reason`,`banishments`.`player_id` AS `player_id`,`banishments`.`banished_by` AS `banished_by`,if(`banishments`.`duration` is null,NULL,(`banishments`.`date_ban` + interval `banishments`.`duration` second) - current_timestamp()) AS `remaining_secs` from `banishments` where `banishments`.`date_ban` + interval `banishments`.`duration` second > current_timestamp() or `banishments`.`duration` is null */;
/*!50001 SET character_set_client      = @saved_cs_client */;
/*!50001 SET character_set_results     = @saved_cs_results */;
/*!50001 SET collation_connection      = @saved_col_connection */;

--
-- Final view structure for view `global_event_records`
--

/*!50001 DROP VIEW IF EXISTS `global_event_records`*/;
/*!50001 SET @saved_cs_client          = @@character_set_client */;
/*!50001 SET @saved_cs_results         = @@character_set_results */;
/*!50001 SET @saved_col_connection     = @@collation_connection */;
/*!50001 SET character_set_client      = utf8mb4 */;
/*!50001 SET character_set_results     = utf8mb4 */;
/*!50001 SET collation_connection      = utf8mb4_general_ci */;
/*!50001 CREATE ALGORITHM=UNDEFINED */
/*!50013 DEFINER=`root`@`localhost` SQL SECURITY DEFINER */
/*!50001 VIEW `global_event_records` AS with record as (select `r`.`record_id` AS `record_id`,`r`.`record_player_id` AS `record_player_id`,`r`.`map_id` AS `map_id`,`r`.`time` AS `time`,`r`.`respawn_count` AS `respawn_count`,`r`.`record_date` AS `record_date`,`r`.`flags` AS `flags`,`r`.`try_count` AS `try_count`,`r`.`event_record_id` AS `event_record_id`,`eer`.`event_id` AS `event_id`,`eer`.`edition_id` AS `edition_id` from (`records` `r` join `event_edition_records` `eer` on(`eer`.`record_id` = `r`.`record_id`)))select `r`.`record_id` AS `record_id`,`r`.`record_player_id` AS `record_player_id`,`r`.`map_id` AS `map_id`,`r`.`time` AS `time`,`r`.`respawn_count` AS `respawn_count`,`r`.`record_date` AS `record_date`,`r`.`flags` AS `flags`,`r`.`try_count` AS `try_count`,`r`.`event_record_id` AS `event_record_id`,`r`.`event_id` AS `event_id`,`r`.`edition_id` AS `edition_id` from ((`record` `r` left join `record` `r3` on(`r3`.`map_id` = `r`.`map_id` and `r3`.`record_player_id` = `r`.`record_player_id` and `r3`.`time` < `r`.`time` and `r3`.`event_id` = `r`.`event_id` and `r3`.`edition_id` = `r`.`edition_id`)) left join `record` `r4` on(`r4`.`map_id` = `r`.`map_id` and `r4`.`record_player_id` = `r`.`record_player_id` and `r4`.`record_id` > `r`.`record_id` and `r4`.`time` = `r`.`time` and `r4`.`event_id` = `r`.`event_id` and `r4`.`edition_id` = `r`.`edition_id`)) where `r3`.`record_id` is null and `r4`.`record_id` is null */;
/*!50001 SET character_set_client      = @saved_cs_client */;
/*!50001 SET character_set_results     = @saved_cs_results */;
/*!50001 SET collation_connection      = @saved_col_connection */;

--
-- Final view structure for view `global_records`
--

/*!50001 DROP VIEW IF EXISTS `global_records`*/;
/*!50001 SET @saved_cs_client          = @@character_set_client */;
/*!50001 SET @saved_cs_results         = @@character_set_results */;
/*!50001 SET @saved_col_connection     = @@collation_connection */;
/*!50001 SET character_set_client      = utf8mb4 */;
/*!50001 SET character_set_results     = utf8mb4 */;
/*!50001 SET collation_connection      = utf8mb4_general_ci */;
/*!50001 CREATE ALGORITHM=UNDEFINED */
/*!50013 DEFINER=`root`@`localhost` SQL SECURITY DEFINER */
/*!50001 VIEW `global_records` AS with eer as (select `eer`.`record_id` AS `record_id`,`ee`.`non_original_maps` AS `non_original_maps` from (`event_edition_records` `eer` join `event_edition` `ee` on(`ee`.`event_id` = `eer`.`event_id` and `ee`.`id` = `eer`.`edition_id`)))select `r`.`record_id` AS `record_id`,`r`.`record_player_id` AS `record_player_id`,`r`.`map_id` AS `map_id`,`r`.`time` AS `time`,`r`.`respawn_count` AS `respawn_count`,`r`.`record_date` AS `record_date`,`r`.`flags` AS `flags`,`r`.`try_count` AS `try_count`,`r`.`event_record_id` AS `event_record_id` from (((`records` `r` left join `records` `r3` on(`r3`.`map_id` = `r`.`map_id` and `r3`.`record_player_id` = `r`.`record_player_id` and `r3`.`time` < `r`.`time`)) left join `records` `r4` on(`r4`.`map_id` = `r`.`map_id` and `r4`.`record_player_id` = `r`.`record_player_id` and `r4`.`record_id` > `r`.`record_id` and `r4`.`time` = `r`.`time`)) left join `eer` on(`eer`.`record_id` = `r`.`record_id`)) where `r3`.`record_id` is null and `r4`.`record_id` is null and (`eer`.`record_id` is null or `eer`.`non_original_maps` <> 0) */;
/*!50001 SET character_set_client      = @saved_cs_client */;
/*!50001 SET character_set_results     = @saved_cs_results */;
/*!50001 SET collation_connection      = @saved_col_connection */;
/*!40103 SET TIME_ZONE=@OLD_TIME_ZONE */;

/*!40101 SET SQL_MODE=@OLD_SQL_MODE */;
/*!40014 SET FOREIGN_KEY_CHECKS=@OLD_FOREIGN_KEY_CHECKS */;
/*!40014 SET UNIQUE_CHECKS=@OLD_UNIQUE_CHECKS */;
/*!40101 SET CHARACTER_SET_CLIENT=@OLD_CHARACTER_SET_CLIENT */;
/*!40101 SET CHARACTER_SET_RESULTS=@OLD_CHARACTER_SET_RESULTS */;
/*!40101 SET COLLATION_CONNECTION=@OLD_COLLATION_CONNECTION */;
/*!40111 SET SQL_NOTES=@OLD_SQL_NOTES */;

-- Dump completed on 2024-08-28 20:06:12
